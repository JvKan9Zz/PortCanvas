"""User-cache preparation tests. No real package manager, SSH, camera or desktop."""
import ast
import contextlib
import importlib.util
import json
import os
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

SOURCE = Path(__file__).resolve().parents[1] / 'src-tauri/resources/prepare_environment.py'
spec = importlib.util.spec_from_file_location('portcanvas_environment_test', SOURCE)
envmod = importlib.util.module_from_spec(spec)
spec.loader.exec_module(envmod)

class EnvironmentTests(unittest.TestCase):
    def test_existing_backend_is_reused_without_creating_cache_or_downloading(self):
        with tempfile.TemporaryDirectory() as temporary:
            home = Path(temporary)
            with patch.object(envmod.Path, 'home', return_value=home), patch.object(envmod, 'ready', return_value=True), patch.object(envmod, 'run', side_effect=AssertionError('must not download')):
                result = envmod.prepare('desktop')
            self.assertTrue(result['ready'])
            self.assertFalse(result['prepared'])
            self.assertFalse((home / '.cache').exists())

    def test_dependency_resolution_keeps_installed_packages_and_handles_alternatives(self):
        catalog = {'x11vnc': 'Package: x11vnc\nVersion: 1\nDepends: libc6 (>= 2), libvncserver1, missing | replacement\n',
                   'libvncserver1': 'Package: libvncserver1\nVersion: 2\nDepends: libc6\n',
                   'replacement': 'Package: replacement\nVersion: 3\nDepends: libvncserver1\n'}
        def run(arguments, **_):
            if arguments[0] == 'dpkg': return 0, 'arm64\n'
            if arguments[0] == 'dpkg-query': return 0, 'libc6\tarm64\tinstalled\nlibvncserver1\tarmhf\tinstalled\n'
            return (0, catalog[arguments[-1]]) if arguments[-1] in catalog else (1, '')
        with patch.object(envmod, 'run', side_effect=run):
            native, packages = envmod.required_packages(['x11vnc'])
        self.assertEqual(native, 'arm64')
        self.assertEqual(packages, {'x11vnc': '1', 'libvncserver1': '2', 'replacement': '3'})
        self.assertNotIn('libc6', packages)

    def test_failed_download_leaves_previous_cache_and_cleans_staging(self):
        with tempfile.TemporaryDirectory() as temporary:
            home = Path(temporary)
            with patch.object(envmod.Path, 'home', return_value=home), patch.object(envmod, 'ready', return_value=False), patch.object(envmod.shutil, 'which', return_value='/fixture/tool'), patch.object(envmod, 'runtime_lock', side_effect=lambda _: contextlib.nullcontext()), patch.object(envmod, 'required_packages', return_value=('arm64', {'x11vnc': '1'})), patch.object(envmod, 'run', side_effect=RuntimeError('synthetic download failure')):
                with self.assertRaisesRegex(RuntimeError, 'synthetic download failure'):
                    envmod.prepare('desktop')
            base = home / '.cache/portcanvas/runtime'
            self.assertFalse((base / 'current.json').exists())
            self.assertEqual(list(base.glob('.prepare-*')), [])

    def test_cache_pointer_cannot_escape_the_managed_versions_directory(self):
        with tempfile.TemporaryDirectory() as temporary:
            base = Path(temporary)
            for identifier in ['../../outside', '', 'invalid', '/absolute']:
                (base / 'current.json').write_text(json.dumps({'id': identifier}))
                self.assertIsNone(envmod.cache_root(base))

    def test_private_wrappers_quote_paths_and_preserve_original_library_environment(self):
        with tempfile.TemporaryDirectory() as temporary:
            tree = Path(temporary)
            (tree / 'usr/bin').mkdir(parents=True)
            (tree / 'usr/bin/tmux').write_bytes(b'synthetic executable')
            destination = Path("/home/fixture's folder/runtime")
            envmod.write_wrappers(tree, destination)
            wrapper = (tree / 'bin/tmux').read_text()
            self.assertIn("fixture'\"'\"'s folder", wrapper)
            self.assertIn('if [ -z "${PORTCANVAS_RUNTIME_LD_SET+x}" ]', wrapper)
            self.assertIn('"$@"', wrapper)

    def test_unknown_feature_cannot_request_arbitrary_packages(self):
        with self.assertRaisesRegex(ValueError, '不支持'):
            envmod.prepare('arbitrary-package')

    def test_helper_syntax_remains_compatible_with_factory_python38(self):
        ast.parse(SOURCE.read_text(encoding='utf-8'), feature_version=(3, 8))

if __name__ == '__main__': unittest.main()
