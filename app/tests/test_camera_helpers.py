"""Synthetic camera backend selection; no V4L2 or capture process is opened."""
import ast
import builtins
import io
import json
import os
import types
import unittest
from pathlib import Path
from unittest.mock import Mock, patch

RESOURCES = Path(__file__).resolve().parents[1] / 'core/resources'

class CameraHelpersTests(unittest.TestCase):
    def select_backend(self, cv2_available, ffmpeg='/fixture/ffmpeg', native=False, import_error=ImportError):
        source = ast.parse((RESOURCES / 'camera_stream.py').read_text(encoding='utf-8'))
        entry = [node for node in source.body if isinstance(node, ast.Try)][-1]
        copied, opencv, stop = Mock(), Mock(), Mock()
        def import_module(name, *args, **kwargs):
            if name == 'cv2':
                if not cv2_available: raise import_error('synthetic unusable cv2')
                return types.SimpleNamespace()
            return builtins.__import__(name, *args, **kwargs)
        namespace = {'__builtins__':dict(vars(builtins), __import__=import_module),
                     'shutil':types.SimpleNamespace(which=lambda _:ffmpeg), 'config':{},
                     'native_mjpeg':lambda:native,'copied_mjpeg':copied,'opencv_preview':opencv,
                     'stop_child':stop,'sys':types.SimpleNamespace(stderr=io.StringIO(),exit=Mock())}
        exec(compile(ast.Module(body=[entry], type_ignores=[]), '<camera-entry>', 'exec'), namespace)
        stop.assert_called_once()
        return copied, opencv
    def test_ffmpeg_encodes_when_opencv_is_missing(self):
        copied, opencv = self.select_backend(False)
        copied.assert_called_once_with('/fixture/ffmpeg', False)
        opencv.assert_not_called()
    def test_existing_opencv_fallback_is_retained(self):
        copied, opencv = self.select_backend(True)
        copied.assert_not_called()
        opencv.assert_called_once()
    def test_unusable_opencv_does_not_require_modifying_the_sdk(self):
        copied, opencv = self.select_backend(False, import_error=RuntimeError)
        copied.assert_called_once_with('/fixture/ffmpeg', False)
        opencv.assert_not_called()
    def test_native_mjpeg_keeps_the_copy_path(self):
        copied, opencv = self.select_backend(False, native=True)
        copied.assert_called_once_with('/fixture/ffmpeg')
        opencv.assert_not_called()
    def test_inaccessible_video_device_is_reported_as_permission_issue(self):
        source = (RESOURCES / 'camera_list.py').read_text(encoding='utf-8')
        output = io.StringIO()
        with patch.dict('sys.modules', {'fcntl':types.SimpleNamespace(ioctl=Mock())}), patch.object(os,'O_NONBLOCK',2048,create=True), patch('glob.glob',return_value=['/dev/video0']), patch('os.open',side_effect=PermissionError()), patch.object(Path,'read_text',return_value='Synthetic camera\n'), patch('sys.stdout',output):
            exec(compile(source, '<camera-list>', 'exec'), {})
        self.assertEqual(json.loads(output.getvalue()), [{'path':'/dev/video0','name':'Synthetic camera','accessible':False}])

if __name__ == '__main__': unittest.main()
