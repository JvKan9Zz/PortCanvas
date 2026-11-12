"""Sign a preview installer with native Minisign and embed its version-bound signature.

Existing Tauri key files are outer-base64 Minisign files; only that wrapper is
decoded temporarily beside the protected source key. No new key is generated.
"""
import argparse
import base64
import json
import os
from pathlib import Path
import subprocess
import tempfile
import time

p = argparse.ArgumentParser()
p.add_argument('--minisign', type=Path, required=True)
p.add_argument('--private-key', type=Path, required=True)
p.add_argument('--public-key', type=Path, required=True)
p.add_argument('--password-file', type=Path, help='Existing protected local signing password; never logged')
p.add_argument('--installer', type=Path, required=True)
p.add_argument('--output', type=Path, required=True)
p.add_argument('--version', required=True)
p.add_argument('--notes', default='')
args = p.parse_args()
installer = args.installer.resolve(strict=True)
key = args.private_key.resolve(strict=True)
public_key = args.public_key.resolve(strict=True)
output = args.output.resolve()
if output.exists():
    raise SystemExit('Preserve the existing manifest; choose a fresh output.')
if not args.version.startswith('0.6.0-beta.'):
    raise SystemExit('This packaging entry point is scoped to the 0.6.0 preview channel.')

def checked(arguments, password=b''):
    result = subprocess.run([str(args.minisign.resolve(strict=True)), *map(str, arguments)], input=password,
                            stdout=subprocess.DEVNULL, stderr=subprocess.PIPE, timeout=60)
    if result.returncode:
        reason = next((label for label in ('Wrong password for that key', 'get_password()', 'Unable to complete key derivation') if label.encode() in result.stderr), 'signing/verification rejected')
        raise SystemExit(f'Minisign operation failed ({result.returncode}): {reason}; private diagnostics were not logged.')

output.parent.mkdir(parents=True, exist_ok=True)
with tempfile.TemporaryDirectory(prefix='portcanvas-sign-', dir=key.parent) as private:
    secret = Path(private) / 'secret.key'
    public = Path(private) / 'public.key'
    signature = Path(private) / 'installer.minisig'
    secret.write_bytes(base64.b64decode(key.read_bytes().strip(), validate=True))
    public.write_bytes(base64.b64decode(public_key.read_bytes().strip(), validate=True))
    comment = f'timestamp:{int(time.time())}\tfile:{installer.name}\tversion:{args.version}'
    value = args.password_file.read_text(encoding='utf-8-sig').rstrip('\r\n') if args.password_file else os.environ.get('PORTCANVAS_SIGNING_PASSWORD', '')
    password = (value + '\n').encode('utf-8')
    checked(['-S', '-s', secret, '-m', installer, '-x', signature, '-t', comment], password)
    checked(['-V', '-p', public, '-m', installer, '-x', signature])
    signed = base64.b64encode(signature.read_bytes()).decode('ascii')
    manifest = {'version':args.version, 'notes':args.notes,
                'platforms':{'windows-x86_64':{
                    'url':f'https://github.com/JvKan9Zz/PortCanvas/releases/download/v{args.version}/{installer.name}',
                    'signature':signed}}}
    output.write_text(json.dumps(manifest, ensure_ascii=False, indent=2) + '\n', encoding='utf-8')
print('Native signature verified and embedded in the preview manifest; temporary key copies removed.')
