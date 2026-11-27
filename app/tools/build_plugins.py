"""Build reproducible .pcplugin packages and the corresponding catalog."""
import argparse
import base64
import gzip
import hashlib
import json
import os
import subprocess
import tarfile
import tempfile
from pathlib import Path
from urllib.parse import quote, urlsplit

ROOT = Path(__file__).resolve().parents[2]
SCHEMA = 'portcanvas.plugin/1'
RELEASE_URL = 'https://github.com/JvKan9Zz/PortCanvas/releases/download/v0.7.0'


def package(source, target):
    with target.open('wb') as output, gzip.GzipFile(filename='', mode='wb', fileobj=output, mtime=0) as compressed:
        with tarfile.open(fileobj=compressed, mode='w', format=tarfile.USTAR_FORMAT) as archive:
            for member in sorted(source.rglob('*')):
                if member.is_symlink():
                    raise ValueError(f'Symbolic link in plugin source: {member}')
                if not member.is_file():
                    continue
                relative = member.relative_to(source)
                if '__pycache__' in relative.parts or member.suffix == '.pyc':
                    continue
                entry = tarfile.TarInfo(relative.as_posix())
                entry.size = member.stat().st_size
                entry.mode = 0o644
                with member.open('rb') as data:
                    archive.addfile(entry, data)


def key_bytes(path):
    data = path.read_bytes().strip()
    return data if data.startswith(b'untrusted comment:') else base64.b64decode(data, validate=True)


def sign(payload, minisign, secret_key, public_key):
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
            result = subprocess.run([str(minisign), *map(str, args)], input=stdin,
                                    capture_output=True, timeout=60)
            if result.returncode:
                raise RuntimeError(f'Minisign failed with exit code {result.returncode}.')
        return base64.b64encode(signature.read_bytes()).decode('ascii')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output-dir', type=Path, default=ROOT / 'app/target/plugins')
    parser.add_argument('--catalog', type=Path, default=ROOT / 'plugins/catalog.json')
    parser.add_argument('--base-url', default=RELEASE_URL)
    parser.add_argument('--minisign', type=Path)
    parser.add_argument('--secret-key', type=Path)
    parser.add_argument('--public-key', type=Path)
    args = parser.parse_args()
    signing = (args.minisign, args.secret_key, args.public_key)
    if any(signing) and not all(signing):
        parser.error('Provide --minisign, --secret-key and --public-key together.')
    base = urlsplit(args.base_url)
    if (not base.hostname or base.username is not None or base.password is not None
            or base.query or base.fragment or not (base.scheme == 'https'
                or (base.scheme == 'http' and base.hostname == '127.0.0.1'))):
        parser.error('Use an HTTPS base URL or HTTP at 127.0.0.1 for local tests.')
    args.output_dir.mkdir(parents=True, exist_ok=True)
    args.catalog.parent.mkdir(parents=True, exist_ok=True)
    entries = []
    with tempfile.TemporaryDirectory(dir=args.output_dir) as work:
        for manifest_path in sorted((ROOT / 'plugins').glob('*/manifest.json')):
            manifest = json.loads(manifest_path.read_text(encoding='utf-8'))
            source = manifest_path.parent
            if manifest['schema'] != SCHEMA or manifest['id'] != source.name:
                raise ValueError(f'Invalid plugin manifest: {manifest_path}')
            filename = f"{manifest['id']}-{manifest['version']}.pcplugin"
            if filename != Path(filename).name or '\\' in filename:
                raise ValueError(f'Invalid package name: {filename}')
            output = Path(work) / filename
            package(source, output)
            with output.open('rb') as data:
                digest = hashlib.file_digest(data, 'sha256').hexdigest()
            entry = dict(manifest, downloadUrl=f"{args.base_url.rstrip('/')}/{quote(filename)}",
                         sizeBytes=output.stat().st_size, sha256=digest)
            if all(signing):
                entry['signature'] = sign(output, *(part.resolve(strict=True) for part in signing))
            entries.append(entry)
        if not entries:
            raise ValueError('No plugin manifests found.')
        for output in Path(work).iterdir():
            output.replace(args.output_dir / output.name)
    with tempfile.NamedTemporaryFile(mode='w', encoding='utf-8', newline='\n',
                                     dir=args.catalog.parent, delete=False) as staged:
        json.dump({'schema': SCHEMA, 'plugins': entries}, staged, ensure_ascii=False, indent=2)
        staged.write('\n')
        pending = Path(staged.name)
    try:
        pending.replace(args.catalog)
    finally:
        pending.unlink(missing_ok=True)
    for entry in entries:
        print(f"{entry['id']} {entry['version']}: {entry['sizeBytes']} bytes")


if __name__ == '__main__':
    main()
