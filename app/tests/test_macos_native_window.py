"""Real AppKit bridge contracts; no window is ordered onto the user's screen."""
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest


@unittest.skipUnless(sys.platform == "darwin", "AppKit is macOS-only")
class NativeWindowContract(unittest.TestCase):
    def test_material_order_modal_opacity_editing_and_cancellable_system_quit(self):
        root = Path(os.environ["WCS_TEST_ROOT"])
        self.assertTrue(root.is_absolute())
        root.mkdir(parents=True, exist_ok=True)
        source = Path(__file__).resolve().parents[1] / "native-probe/native/macos/window_contract.m"
        with tempfile.TemporaryDirectory(prefix="window-contract-", dir=root) as folder:
            binary = Path(folder) / "contract"
            subprocess.run(["xcrun", "clang", "-mmacosx-version-min=13.0", "-fobjc-arc", "-fmodules", "-Wno-deprecated-declarations", str(source), "-framework", "AppKit", "-framework", "QuartzCore", "-framework", "CoreGraphics", "-o", str(binary)], check=True, capture_output=True, text=True)
            result = subprocess.run([str(binary)], check=True, capture_output=True, text=True, timeout=20)
            self.assertIn("Native AppKit window contracts passed.", result.stdout)
