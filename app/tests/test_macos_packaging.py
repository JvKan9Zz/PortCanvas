"""Packaging contracts; Mach-O/DMG integration runs on macOS CI separately."""
import importlib.util
import os
import tarfile
import tempfile
import unittest
from pathlib import Path

SCRIPT = Path(__file__).resolve().parents[1] / 'tools' / 'macos_package.py'


class MacPackagingTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        spec = importlib.util.spec_from_file_location('macos_package', SCRIPT)
        cls.package = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(cls.package)

    def test_stable_and_beta_have_distinct_install_and_signed_version_metadata(self):
        stable = self.package.bundle_metadata('0.6.1')
        beta = self.package.bundle_metadata('0.6.2-beta.5')
        self.assertEqual(stable['CFBundleIdentifier'], 'com.portcanvas.desktop')
        self.assertEqual(beta['CFBundleIdentifier'], 'com.portcanvas.desktop.beta')
        self.assertEqual(beta['CFBundleDisplayName'], 'PortCanvas Beta')
        self.assertEqual(beta['CFBundleShortVersionString'], '0.6.2')
        self.assertEqual(beta['CFBundleVersion'], '0.6.2b5')
        self.assertEqual(beta['PortCanvasVersion'], '0.6.2-beta.5')
        self.assertNotEqual(stable['CFBundleName'], beta['CFBundleName'])

    def test_prerelease_versions_follow_the_supported_apple_build_counter(self):
        self.assertEqual(self.package.bundle_metadata('0.6.2-beta.255')['CFBundleVersion'], '0.6.2b255')
        for version in ['0.6.2-beta.0', '0.6.2-beta.256', '0.6.2-beta.01', '0.6.2-alpha.1', '0.6.2+local', '0.6２.2', '0.6.2-beta.1２']:
            with self.subTest(version=version):
                with self.assertRaises(ValueError):
                    self.package.bundle_metadata(version)

    def test_minimum_os_parser_checks_every_slice(self):
        loads = 'cmd LC_BUILD_VERSION\n platform 1\n minos 13.0\n sdk 26.0\n'
        self.assertEqual(self.package.minimum_versions(loads), ['13.0'])
        legacy = 'cmd LC_VERSION_MIN_MACOSX\n cmdsize 16\n version 13.0\n sdk 15.0\n'
        self.assertEqual(self.package.minimum_versions(legacy), ['13.0'])
        self.assertEqual(self.package.minimum_versions(''), [])

    def test_non_system_dynamic_dependencies_are_rejected(self):
        self.assertTrue(self.package.is_system_dependency('/System/Library/Frameworks/AppKit.framework/Versions/C/AppKit'))
        self.assertTrue(self.package.is_system_dependency('/usr/lib/libSystem.B.dylib'))
        for dependency in ['@rpath/libcrypto.dylib', '/opt/homebrew/lib/libssl.dylib', '/usr/lib/../../tmp/injected.dylib']:
            self.assertFalse(self.package.is_system_dependency(dependency), dependency)

    def test_archive_has_single_root_and_normalized_metadata(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            app = root / 'PortCanvas.app'
            binary = app / 'Contents' / 'MacOS' / 'PortCanvasNative'
            binary.parent.mkdir(parents=True)
            binary.write_bytes(b'test fixture, not an application')
            binary.chmod(0o755)
            first, second = root / 'first.tar.gz', root / 'second.tar.gz'
            self.package.create_archive(app, first, 1234567890)
            self.package.create_archive(app, second, 1234567890)
            self.assertEqual(first.read_bytes(), second.read_bytes())
            with tarfile.open(first) as archive:
                members = archive.getmembers()
                self.assertTrue(all(m.name == 'PortCanvas.app' or m.name.startswith('PortCanvas.app/') for m in members))
                self.assertTrue(all(m.isfile() or m.isdir() for m in members))
                self.assertTrue(all(m.uid == 0 and m.gid == 0 and m.mtime == 1234567890 for m in members))
                self.assertEqual(archive.extractfile('PortCanvas.app/Contents/MacOS/PortCanvasNative').read(), binary.read_bytes())

    @unittest.skipIf(os.name == "nt", "Windows symlink creation depends on host privilege; exercised on macOS")
    def test_archive_rejects_links(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            app = root / 'PortCanvas.app'
            app.mkdir()
            (app / 'escape').symlink_to('/etc/passwd')
            with self.assertRaisesRegex(ValueError, 'link|regular'):
                self.package.create_archive(app, root / 'bad.tar.gz', 0)

    def test_output_must_be_new_and_parent_exists(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            self.assertEqual(self.package.new_output_path(root / 'new'), root / 'new')
            with self.assertRaisesRegex(ValueError, 'exist'):
                self.package.new_output_path(root)
            with self.assertRaisesRegex(ValueError, 'parent'):
                self.package.new_output_path(root / 'missing' / 'new')


if __name__ == '__main__':
    unittest.main()
