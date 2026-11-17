#!/usr/bin/env python3
"""Run Mac tests with an exact disposable-Keychain ownership ledger, or clean it."""
import argparse
import hashlib
import json
import os
import re
import subprocess
import sys
from pathlib import Path

PREFIX = 'io.portcanvas.test.credentials.'


def validate_namespace(namespace):
    if not namespace.startswith(PREFIX) or not re.fullmatch(r'[A-Za-z0-9_.-]{1,120}', namespace[len(PREFIX):]):
        raise ValueError('A disposable test namespace is required; production credentials are excluded')


def owned_service(service, namespace):
    validate_namespace(namespace)
    return bool(re.fullmatch(re.escape(namespace) + r'\.[0-9a-f]{16}', service) or
                re.fullmatch(re.escape(PREFIX) + r'[0-9a-f]{8}(?:-[0-9a-f]{4}){3}-[0-9a-f]{12}', service))


def executable_service(executable, target, namespace):
    validate_namespace(namespace)
    executable, target = Path(executable).resolve(), Path(target).resolve()
    try:
        executable.relative_to(target)
    except ValueError:
        raise ValueError(f'Test executable is outside the isolated Cargo target: {executable}')
    if not executable.is_file():
        raise ValueError(f'Test executable is not a regular file: {executable}')
    return namespace + '.' + hashlib.sha256(os.fsencode(executable)).hexdigest()[:16]


def read_ledger(ledger, namespace):
    if ledger.is_symlink():
        raise ValueError('Keychain ownership ledger must not be a link')
    if not ledger.exists():
        return []
    if not ledger.is_file() or (hasattr(os, 'getuid') and ledger.stat().st_uid != os.getuid()):
        raise ValueError('Keychain ownership ledger must be an owned regular file')
    services = set()
    for line in ledger.read_text(encoding='utf-8').splitlines():
        fields = line.split('\t')
        if len(fields) != 2 or not fields[0].isdigit() or not owned_service(fields[1], namespace):
            raise ValueError('Unowned or malformed Keychain ledger entry; cleanup stopped')
        services.add(fields[1])
    return sorted(services)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('action', choices=('test', 'cleanup'))
    parser.add_argument('cargo_args', nargs=argparse.REMAINDER)
    args = parser.parse_args()
    if sys.platform != 'darwin':
        parser.error('This helper only accesses macOS disposable test credentials')
    namespace = os.environ.get('PORTCANVAS_TEST_KEYCHAIN_SERVICE', '')
    validate_namespace(namespace)
    root = Path(os.environ.get('WCS_TEST_ROOT', ''))
    if not root.is_absolute() or not root.is_dir() or root.is_symlink():
        parser.error('WCS_TEST_ROOT must be an existing isolated absolute directory')
    ledger = root / 'test-keychain-items.tsv'
    # Validate the complete existing ledger before adding or deleting anything.
    read_ledger(ledger, namespace)
    if args.action == 'test':
        target = Path(os.environ['CARGO_TARGET_DIR'])
        if not target.is_absolute():
            parser.error('CARGO_TARGET_DIR must be absolute for executable ownership records')
        command = ['cargo', 'test', '--message-format=json', *args.cargo_args]
        process = subprocess.Popen(command, stdout=subprocess.PIPE, text=True)
        assert process.stdout is not None
        for line in process.stdout:
            try:
                event = json.loads(line)
            except ValueError:
                print(line, end='', flush=True)
                continue
            if isinstance(event, dict) and event.get('reason') == 'compiler-artifact' and event.get('executable'):
                service = executable_service(event['executable'], target, namespace)
                descriptor = os.open(ledger, os.O_WRONLY | os.O_CREAT | os.O_APPEND, 0o600)
                try:
                    os.write(descriptor, f'{os.getpid()}\t{service}\n'.encode())
                finally:
                    os.close(descriptor)
            elif isinstance(event, dict) and event.get('reason') == 'compiler-message':
                print(event.get('message', {}).get('rendered', ''), end='', flush=True)
            else:
                print(line, end='', flush=True)
        raise SystemExit(process.wait())
    if args.cargo_args:
        parser.error('cleanup accepts no extra arguments')
    services = read_ledger(ledger, namespace)
    for service in services:
        result = subprocess.run(['/usr/bin/security', 'delete-generic-password', '-s', service, '-a', 'encryption-key-v1'], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        if result.returncode not in (0, 44):
            raise SystemExit(f'Could not remove owned test Keychain item: {service} (exit {result.returncode})')
        check = subprocess.run(['/usr/bin/security', 'find-generic-password', '-s', service, '-a', 'encryption-key-v1'], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        if check.returncode != 44:
            raise SystemExit(f'Owned test Keychain item absence unverified: {service} (exit {check.returncode})')
    print(f'Confirmed absent: {len(services)} exact owned test Keychain services; ledger retained at {ledger}')


if __name__ == '__main__':
    main()
