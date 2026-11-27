import hashlib
import json
import subprocess
import sys
import tarfile
import tempfile
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]


class PluginPackagerTests(unittest.TestCase):
    def test_reproducible_packages_match_catalog_and_contain_runtime_files(self):
        with tempfile.TemporaryDirectory() as scratch:
            scratch = Path(scratch)
            catalog = scratch / 'catalog.json'
            argv = [sys.executable, '-B', str(ROOT / 'app/tools/build_plugins.py'),
                    '--catalog', str(catalog), '--base-url', 'http://127.0.0.1:8812']
            for folder in ('first', 'second'):
                subprocess.run([*argv, '--output-dir', str(scratch / folder)], check=True, capture_output=True)
            data = json.loads(catalog.read_text(encoding='utf-8'))
            self.assertEqual(data['schema'], 'portcanvas.plugin/1')
            self.assertEqual({entry['id'] for entry in data['plugins']}, {'arena-monitor', 'vision-studio'})
            for entry in data['plugins']:
                with self.subTest(plugin=entry['id']):
                    filename = f"{entry['id']}-{entry['version']}.pcplugin"
                    payload = scratch / 'second' / filename
                    package = payload.read_bytes()
                    self.assertEqual(package, (scratch / 'first' / filename).read_bytes())
                    self.assertEqual(entry['downloadUrl'], f'http://127.0.0.1:8812/{filename}')
                    self.assertEqual(entry['sizeBytes'], len(package))
                    self.assertEqual(entry['sha256'], hashlib.sha256(package).hexdigest())
                    with tarfile.open(payload, 'r:gz') as archive:
                        self.assertTrue(all(member.isfile() for member in archive.getmembers()))
                        manifest = json.load(archive.extractfile('manifest.json'))
                        self.assertEqual(manifest['id'], entry['id'])
                        for name in [*manifest.get('assets', {}).values(), *manifest['scripts'].values()]:
                            with archive.extractfile(name) as content:
                                self.assertEqual(content.read(), (ROOT / 'plugins' / entry['id'] / name).read_bytes())
