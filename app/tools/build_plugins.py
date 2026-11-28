"""Build reproducible .pcplugin packages (multi-platform & declarative-v1) and catalog.json."""
import argparse
import base64
import gzip
import hashlib
import json
import os
import shutil
import subprocess
import tarfile
import tempfile
from pathlib import Path
from urllib.parse import quote, urlsplit

ROOT = Path(__file__).resolve().parents[2]
SCHEMA = 'portcanvas.plugin/1'
RELEASE_URL = 'https://github.com/JvKan9Zz/PortCanvas/releases/download/v0.7.3'


def package(source: Path, target: Path) -> None:
    with target.open('wb') as output, gzip.GzipFile(
        filename='', mode='wb', fileobj=output, mtime=0
    ) as compressed:
        with tarfile.open(fileobj=compressed, mode='w', format=tarfile.USTAR_FORMAT) as archive:
            for member in sorted(source.rglob('*')):
                if member.is_symlink():
                    raise ValueError(f'Symbolic link in plugin source: {member}')
                if not member.is_file():
                    continue
                relative = member.relative_to(source)
                if (
                    '__pycache__' in relative.parts
                    or member.suffix == '.pyc'
                    or relative.parts[0] == 'worker'
                ):
                    continue
                entry = tarfile.TarInfo(relative.as_posix())
                entry.size = member.stat().st_size
                entry.mode = 0o755 if relative.parts[0] == 'bin' else 0o644
                with member.open('rb') as data:
                    archive.addfile(entry, data)


def key_bytes(path: Path) -> bytes:
    data = path.read_bytes().strip()
    return data if data.startswith(b'untrusted comment:') else base64.b64decode(data, validate=True)


def sign(payload: Path, minisign: Path, secret_key: Path, public_key: Path) -> str:
    with tempfile.TemporaryDirectory(prefix='portcanvas-plugin-sign-', dir=secret_key.parent) as work:
        secret = Path(work) / 'secret.key'
        public = Path(work) / 'public.key'
        signature = Path(work) / 'package.minisig'
        secret.write_bytes(key_bytes(secret_key))
        public.write_bytes(key_bytes(public_key))
        password = (os.environ.get('PORTCANVAS_SIGNING_PASSWORD', '') + '\n').encode()
        for args, stdin in (
            (['-S', '-s', secret, '-m', payload, '-x', signature], password),
            (['-V', '-p', public, '-m', payload, '-x', signature], b''),
        ):
            result = subprocess.run(
                [str(minisign), *map(str, args)],
                input=stdin,
                capture_output=True,
                timeout=60,
            )
            if result.returncode:
                raise RuntimeError(f'Minisign failed with exit code {result.returncode}.')
        return base64.b64encode(signature.read_bytes()).decode('ascii')


def parse_worker_binaries(specs: list[str]) -> dict[str, dict[str, Path]]:
    mapping: dict[str, dict[str, Path]] = {}
    for spec in specs:
        left, sep, raw_path = spec.partition('=')
        if not sep or ':' not in left:
            raise ValueError(f'Invalid --worker-binary format (expected id:platform=path): {spec}')
        plugin_id, platform = left.split(':', 1)
        path = Path(raw_path).resolve(strict=True)
        mapping.setdefault(plugin_id, {})[platform] = path
    return mapping


def copy_runtime_files(source: Path, dest: Path) -> None:
    for item in sorted(source.rglob('*')):
        if item.is_symlink() or not item.is_file():
            continue
        rel = item.relative_to(source)
        if rel.parts[0] in ('worker', 'bin') or '__pycache__' in rel.parts or item.suffix == '.pyc':
            continue
        target = dest / rel
        target.parent.mkdir(parents=True, exist_ok=True)
        shutil.copy2(item, target)


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output-dir', type=Path, default=ROOT / 'app/target/plugins')
    parser.add_argument('--catalog', type=Path, default=ROOT / 'plugins/catalog.json')
    parser.add_argument('--base-url', default=RELEASE_URL)
    parser.add_argument(
        '--worker-binary',
        action='append',
        default=[],
        help='Attach compiled local-worker binary: <plugin-id>:<platform>=<path>',
    )
    parser.add_argument('--minisign', type=Path)
    parser.add_argument('--secret-key', type=Path)
    parser.add_argument('--public-key', type=Path)
    args = parser.parse_args()

    signing = (args.minisign, args.secret_key, args.public_key)
    if any(signing) and not all(signing):
        parser.error('Provide --minisign, --secret-key and --public-key together.')
    base = urlsplit(args.base_url)
    if (
        not base.hostname
        or base.username is not None
        or base.password is not None
        or base.query
        or base.fragment
        or not (
            base.scheme == 'https'
            or (base.scheme == 'http' and base.hostname == '127.0.0.1')
        )
    ):
        parser.error('Use an HTTPS base URL or HTTP at 127.0.0.1 for local tests.')

    worker_binaries = parse_worker_binaries(args.worker_binary)
    args.output_dir.mkdir(parents=True, exist_ok=True)
    args.catalog.parent.mkdir(parents=True, exist_ok=True)
    entries = []

    with tempfile.TemporaryDirectory(dir=args.output_dir) as work:
        work_path = Path(work)
        for manifest_path in sorted((ROOT / 'plugins').glob('*/manifest.json')):
            source = manifest_path.parent
            if source.name.startswith('_'):
                continue
            manifest = json.loads(manifest_path.read_text(encoding='utf-8'))
            if manifest['schema'] != SCHEMA or manifest['id'] != source.name:
                raise ValueError(f'Invalid plugin manifest: {manifest_path}')
            plugin_id = manifest['id']
            runtime = manifest.get('runtime') or {}

            if runtime.get('kind') == 'local-worker':
                binaries = worker_binaries.get(plugin_id, {})
                if not binaries:
                    continue
                packages = {}
                for platform, binary_path in sorted(binaries.items()):
                    stage = work_path / f'stage-{plugin_id}-{platform}'
                    copy_runtime_files(source, stage)
                    bin_name = binary_path.name
                    bin_dest = stage / 'bin' / bin_name
                    bin_dest.parent.mkdir(parents=True, exist_ok=True)
                    shutil.copy2(binary_path, bin_dest)
                    pkg_manifest = json.loads(json.dumps(manifest))
                    pkg_manifest['platform'] = platform
                    pkg_manifest['runtime']['entry'] = f'bin/{bin_name}'
                    (stage / 'manifest.json').write_text(
                        json.dumps(pkg_manifest, ensure_ascii=False, indent=2) + '\n',
                        encoding='utf-8',
                    )
                    filename = f"{plugin_id}-{manifest['version']}-{platform}.pcplugin"
                    output = work_path / filename
                    package(stage, output)
                    with output.open('rb') as data:
                        digest = hashlib.file_digest(data, 'sha256').hexdigest()
                    pkg_entry = {
                        'downloadUrl': f"{args.base_url.rstrip('/')}/{quote(filename)}",
                        'sizeBytes': output.stat().st_size,
                        'sha256': digest,
                    }
                    if all(signing):
                        pkg_entry['signature'] = sign(
                            output, *(part.resolve(strict=True) for part in signing)
                        )
                    packages[platform] = pkg_entry
                catalog_manifest = json.loads(json.dumps(manifest))
                catalog_manifest['platform'] = 'any' if len(packages) > 1 else next(iter(packages))
                primary_pkg = packages.get('windows-x86_64') or next(iter(packages.values()))
                entry = dict(catalog_manifest, **primary_pkg, packages=packages)
                entries.append(entry)
            else:
                filename = f"{plugin_id}-{manifest['version']}.pcplugin"
                if filename != Path(filename).name or '\\' in filename:
                    raise ValueError(f'Invalid package name: {filename}')
                output = work_path / filename
                package(source, output)
                with output.open('rb') as data:
                    digest = hashlib.file_digest(data, 'sha256').hexdigest()
                pkg_entry = {
                    'downloadUrl': f"{args.base_url.rstrip('/')}/{quote(filename)}",
                    'sizeBytes': output.stat().st_size,
                    'sha256': digest,
                }
                if all(signing):
                    pkg_entry['signature'] = sign(
                        output, *(part.resolve(strict=True) for part in signing)
                    )
                entry = dict(manifest, **pkg_entry, packages={'any': pkg_entry})
                entries.append(entry)

        if not entries:
            raise ValueError('No plugin manifests found.')
        for output in work_path.iterdir():
            if output.is_file() and output.suffix == '.pcplugin':
                output.replace(args.output_dir / output.name)

    with tempfile.NamedTemporaryFile(
        mode='w',
        encoding='utf-8',
        newline='\n',
        dir=args.catalog.parent,
        delete=False,
    ) as staged:
        json.dump({'schema': SCHEMA, 'plugins': entries}, staged, ensure_ascii=False, indent=2)
        staged.write('\n')
        pending = Path(staged.name)
    try:
        pending.replace(args.catalog)
    finally:
        pending.unlink(missing_ok=True)

    for entry in entries:
        if 'packages' in entry:
            summary = ', '.join(
                f"{plat}: {pkg['sizeBytes']}B" for plat, pkg in entry['packages'].items()
            )
            print(f"{entry['id']} {entry['version']}: {summary}")
        else:
            print(f"{entry['id']} {entry['version']}: {entry['sizeBytes']} bytes")


if __name__ == '__main__':
    main()
