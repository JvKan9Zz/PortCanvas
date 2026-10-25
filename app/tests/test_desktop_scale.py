"""Exercise framebuffer sizing without executing the SSH process entrypoint."""
import ast
import ctypes
import ctypes.util
import os
import unittest
from fractions import Fraction
from pathlib import Path
from unittest.mock import Mock, patch

source=Path(__file__).resolve().parents[1]/'src-tauri/resources/desktop.py'
function=next(node for node in ast.parse(source.read_text(encoding='utf-8')).body if isinstance(node,ast.FunctionDef) and node.name=='scale_options')
namespace={'ctypes':ctypes,'os':os,'Fraction':Fraction}
exec(compile(ast.Module(body=[function],type_ignores=[]),str(source),'exec'),namespace)
scale_options=namespace['scale_options']

class DesktopScaleTests(unittest.TestCase):
    def library(self,width=1920,height=1080):
        return Mock(XOpenDisplay=Mock(return_value=7),XCloseDisplay=Mock(),XDefaultScreen=Mock(return_value=0),XDisplayWidth=Mock(return_value=width),XDisplayHeight=Mock(return_value=height))

    def test_widescreen_and_portrait_fit_without_upscaling(self):
        for width,height,expected in [(1920,1080,'1/2:nb,nocr'),(1280,720,'3/4:nb,nocr'),(1080,1920,'9/32:nb,nocr'),(800,450,None)]:
            with self.subTest(size=(width,height)),patch('ctypes.util.find_library',return_value='libX11.so'),patch('ctypes.CDLL',return_value=self.library(width,height)) as load,patch.dict(os.environ):
                self.assertEqual(scale_options(':0','/test/auth',960,540),['-scale',expected] if expected else [])
                load.return_value.XCloseDisplay.assert_called_once_with(7)
                self.assertEqual(os.environ['XAUTHORITY'],'/test/auth')

    def test_missing_library_or_display_keeps_the_native_vnc_path(self):
        with patch('ctypes.util.find_library',return_value=None):
            self.assertEqual(scale_options(':0','/test/auth',960,540),[])
        library=self.library();library.XOpenDisplay.return_value=None
        with patch('ctypes.util.find_library',return_value='libX11.so'),patch('ctypes.CDLL',return_value=library),patch.dict(os.environ):
            self.assertEqual(scale_options(':0','/test/auth',960,540),[])
            library.XCloseDisplay.assert_not_called()

    def test_query_failure_closes_display_and_preserves_native_fallback(self):
        library=self.library();library.XDisplayWidth.side_effect=OSError('query failed')
        with patch('ctypes.util.find_library',return_value='libX11.so'),patch('ctypes.CDLL',return_value=library),patch.dict(os.environ):
            self.assertEqual(scale_options(':0','/test/auth',960,540),[])
            library.XCloseDisplay.assert_called_once_with(7)

if __name__=='__main__':unittest.main()
