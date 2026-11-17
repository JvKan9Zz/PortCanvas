"""Ownership boundaries for CI's disposable Keychain ledger; no Keychain access."""
import importlib.util
import tempfile
import unittest
from pathlib import Path


class TestKeychainLedger(unittest.TestCase):
    def setUp(self):
        path = Path(__file__).resolve().parents[1] / 'tools/macos_test_keychain.py'
        self.assertTrue(path.is_file(), 'CI needs the disposable Keychain ownership helper')
        spec = importlib.util.spec_from_file_location('macos_test_keychain', path)
        self.helper = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(self.helper)
        self.namespace = 'io.portcanvas.test.credentials.ci-123-1-universal'

    def test_namespace_rejects_production_or_unsafe_names(self):
        self.helper.validate_namespace(self.namespace)
        for name in ['io.portcanvas.app.credentials', 'io.portcanvas.test.credentials.', self.namespace + '/escape']:
            with self.assertRaises(ValueError): self.helper.validate_namespace(name)

    def test_owned_services_require_exact_run_hash_or_uuid_ledger_entry(self):
        for service in [self.namespace + '.0123456789abcdef', 'io.portcanvas.test.credentials.12345678-1234-1234-1234-123456789abc']:
            self.assertTrue(self.helper.owned_service(service, self.namespace))
        for service in ['io.portcanvas.app.credentials', self.namespace, self.namespace + '.bad', 'io.portcanvas.test.credentials.other.0123456789abcdef']:
            self.assertFalse(self.helper.owned_service(service, self.namespace))

    def test_executable_service_matches_rust_digest_and_is_under_target(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            binary = root / 'debug/deps/test fixture'
            binary.parent.mkdir(parents=True)
            binary.write_text('fixture')
            import hashlib, os
            expected = self.namespace + '.' + hashlib.sha256(os.fsencode(binary.resolve())).hexdigest()[:16]
            self.assertEqual(self.helper.executable_service(binary, root, self.namespace), expected)
            with self.assertRaises(ValueError): self.helper.executable_service(root.parent, root, self.namespace)

    def test_ledger_rejects_unowned_or_malformed_rows_before_cleanup(self):
        with tempfile.TemporaryDirectory() as directory:
            ledger = Path(directory) / 'test-keychain-items.tsv'
            service = self.namespace + '.0123456789abcdef'
            ledger.write_text('123\t' + service + '\n123\t' + service + '\n')
            self.assertEqual(self.helper.read_ledger(ledger, self.namespace), [service])
            for value in ['123\tio.portcanvas.app.credentials\n', 'not-a-pid\t' + service + '\n', '123\t' + service + '\textra\n']:
                ledger.write_text(value)
                with self.assertRaises(ValueError): self.helper.read_ledger(ledger, self.namespace)


if __name__ == '__main__': unittest.main()
