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
                        expected_files = [*manifest.get('assets', {}).values(), *manifest.get('scripts', {}).values()]
                        if manifest.get('ui'):
                            expected_files.append(manifest['ui'])
                        for name in expected_files:
                            with archive.extractfile(name) as content:
                                self.assertEqual(content.read(), (ROOT / 'plugins' / entry['id'] / name).read_bytes())

    def test_developer_template_packs_with_working_worker_script(self):
        template_dir = ROOT / 'plugins/_template'
        with tempfile.TemporaryDirectory() as scratch:
            out_pkg = Path(scratch) / 'hello-toolbox-0.1.0.pcplugin'
            subprocess.run(
                [sys.executable, '-B', str(template_dir / 'pack.py'), str(template_dir), str(out_pkg)],
                check=True,
                capture_output=True,
            )
            with tarfile.open(out_pkg, 'r:gz') as archive:
                names = set(archive.getnames())
                self.assertIn('manifest.json', names)
                self.assertIn('ui.json', names)
                self.assertIn('bin/worker.py', names)
            proc = subprocess.run(
                [sys.executable, '-u', str(template_dir / 'bin/worker.py')],
                input='{"type":"init"}\n{"type":"set_state","key":"message","value":"hi"}\n{"type":"action","action":"ping"}\n{"type":"shutdown"}\n',
                text=True,
                encoding='utf-8',
                capture_output=True,
                check=True,
                timeout=10,
            )
            lines = [json.loads(line) for line in proc.stdout.splitlines() if line.strip()]
            self.assertTrue(any(m.get('type') == 'state_patch' and m.get('patch', {}).get('count') == 1 and m.get('patch', {}).get('lastEcho') == 'hi' for m in lines))
