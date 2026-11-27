"""Vision protocol and routing tests use synthetic inputs without a camera or X server."""
import ast
import importlib.util
import json
import os
import struct
import sys
import tempfile
import time
import types
import unittest
from pathlib import Path
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[2] / 'plugins/vision-studio/scripts'

try:
    import cv2
    import numpy as np
except ImportError:
    cv2 = np = None


def load(name):
    spec = importlib.util.spec_from_file_location(name, ROOT / (name + '.py'))
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


INSPECT = load('vision_inspect')
with patch.dict(sys.modules, {'vision_inspect': INSPECT}):
    STREAM = load('vision_stream')


class VisionProtocolTests(unittest.TestCase):
    def test_board_sources_parse_as_python_38(self):
        for path in ROOT.glob('*.py'):
            ast.parse(path.read_text(encoding='utf-8'), filename=str(path), feature_version=(3, 8))

    def test_controls_accept_fragmented_json_along_with_credit_and_heartbeat(self):
        controls = STREAM.Controls({'source': 'auto'})
        controls.credit = False
        message = json.dumps({'cmd': 'config', 'config': {
            'source': 'watch', 'algo': 7, 'blur': 6, 'windowId': '0x1001', 'fps': 20
        }}).encode()
        controls.feed(b'.' + message[:17])
        self.assertEqual(controls.config['source'], 'auto')
        controls.feed(message[17:] + b'\nr.')
        self.assertTrue(controls.credit)
        self.assertEqual(controls.config['source'], 'watch')
        self.assertEqual((controls.config['algo'], controls.config['blur'], controls.config['fps']), (6, 7, 20))
        self.assertEqual(controls.config['windowId'], '0x1001')
        self.assertEqual(STREAM.normalized({'path': '/dev/../../../etc/passwd'})['path'], '')
        with self.assertRaises(ValueError):
            controls.feed(b'{' + b'x' * STREAM.INPUT_LIMIT)

    def test_sources_respect_direct_shared_window_and_screen_priority(self):
        windows = [{'id':'screen','title':'Screen'}, {'id':'0x31','title':'OpenCV Preview'},
                   {'id':'0x42','title':'Lab script'}]
        base = STREAM.normalized({'path': '/dev/video2'})
        self.assertEqual(STREAM.choose_source(base, False, set(), windows), ('camera', '/dev/video2'))
        self.assertEqual(STREAM.choose_source(base, False, {42}, windows), ('x11', '0x31'))
        self.assertEqual(STREAM.choose_source(base, True, {42}, windows), ('shared', INSPECT.SHARED_FRAME))
        direct = dict(base, source='camera')
        self.assertEqual(STREAM.choose_source(direct, True, {42}, windows), ('camera', '/dev/video2'))
        watch = dict(base, source='watch', windowId='0x42')
        self.assertEqual(STREAM.choose_source(watch, False, set(), windows), ('x11', '0x42'))
        self.assertEqual(STREAM.choose_source(watch, False, set(), windows[:1]), ('x11', 'screen'))
        self.assertEqual(STREAM.choose_source(watch, False, set(), []), ('waiting', ''))

    def test_window_tree_keeps_named_clients_and_shared_age_is_bounded(self):
        tree = '''
      0x10 (has no name): ()  660x500+0+0  +10+20
         0x11 "OpenCV Camera": ("python" "Python")  640x480+10+20  +20+40
            0x12 "Child": () 100x100+0+0  +20+40
      0x13 "Other window": ()  320x240+0+0  -50+80
'''
        windows = INSPECT.parse_windows(tree)
        self.assertEqual([row['id'] for row in windows], ['0x11', '0x13'])
        self.assertEqual((windows[-1]['width'], windows[-1]['x']), (320, -50))
        with tempfile.TemporaryDirectory() as root:
            path = Path(root) / 'frame.jpg'
            self.assertFalse(INSPECT.shared_frame(str(path))['available'])
            path.write_bytes(b'example')
            self.assertTrue(INSPECT.shared_frame(str(path))['available'])
            stale = time.time() - 4
            os.utime(path, (stale, stale))
            self.assertFalse(INSPECT.shared_frame(str(path))['available'])

    def test_only_viewable_named_windows_are_offered_beside_screen(self):
        tree = ('    0x11 "Live": () 640x480+0+0 +20+40\n'
                '    0x12 "Minimized": () 640x480+0+0 +20+40\n')
        geometry = {'width':640, 'height':480, 'x':0, 'y':0}
        def window_info(window_id, _env, _timeout=0.8):
            return None if window_id == '0x12' else geometry
        with (patch.object(INSPECT.shutil, 'which', return_value='xwininfo'),
              patch.object(INSPECT, 'window_details', side_effect=window_info),
              patch.object(INSPECT, 'command_output', return_value=tree)):
            self.assertEqual([row['id'] for row in INSPECT.display_windows({'DISPLAY':':0'})],
                             ['screen', '0x11'])

    def test_process_camera_ownership_ignores_preview_pid_and_redacts_arguments(self):
        with tempfile.TemporaryDirectory() as root:
            proc = Path(root)
            for pid in (42, 43):
                fd = proc / str(pid) / 'fd'
                fd.mkdir(parents=True)
                (fd / '2').touch()
            with patch.object(INSPECT.os, 'readlink', return_value='/dev/video0'):
                ids = INSPECT.holder_ids(['/dev/video0'], proc, ignore_pid=43, fallback=False)
        self.assertEqual(ids, {'/dev/video0': {42}})
        command = INSPECT.sanitized_command(b'python\0camera.py\0--password\0sensitive\0--token=hidden\0')
        self.assertIn('camera.py', command)
        self.assertNotIn('sensitive', command)
        self.assertNotIn('hidden', command)


@unittest.skipIf(cv2 is None, 'OpenCV and NumPy are optional in desktop Python')
class VisionImageTests(unittest.TestCase):
    def test_all_algorithms_accept_synthetic_images_and_hsv_reports_line_geometry(self):
        img = np.zeros((240, 320, 3), dtype=np.uint8)
        cv2.rectangle(img, (180, 195), (218, 235), (0, 0, 255), -1)
        cv2.circle(img, (58, 50), 22, (255, 255, 255), 3)
        cv2.rectangle(img, (110, 40), (125, 61), (255, 255, 255), -1)
        cv2.setNumThreads(1)
        tag = types.SimpleNamespace(center=np.array([58., 50.]), tag_id=3,
                                    corners=np.array([[40., 32.], [76., 32.], [76., 68.], [40., 68.]]))
        detector = types.SimpleNamespace(detect=lambda _: [tag])
        apriltag = types.SimpleNamespace(Detector=lambda _: detector, DetectorOptions=lambda **kwargs: kwargs)
        algorithms = STREAM.Algorithms(cv2, np)
        with patch.dict(sys.modules, {'apriltag': apriltag}):
            for algo in range(7):
                with self.subTest(algo=algo):
                    config = STREAM.normalized({'algo': algo, 'lineRoi': True})
                    output, details = algorithms.apply(img.copy(), config)
                    self.assertEqual(output.shape[:2], (240, 320))
                    self.assertTrue(cv2.imencode('.jpg', output)[0])
                    if algo == 3:
                        facts = dict(details)
                        self.assertGreater(float(facts['偏移'].split()[0]), 0)
                        self.assertIn('199', facts['质心'])
                    if algo == 5:
                        self.assertEqual(dict(details)['标签 ID'], '3')
                    if algo == 6:
                        facts = dict(details)
                        if '人脸' not in facts:
                            self.assertEqual(facts['状态'], '缺少 Haar 人脸分类器')
        for feature in (1, 2):
            result, _ = algorithms.apply(img.copy(), STREAM.normalized({'algo': 2, 'featureMode': feature}))
            self.assertEqual(result.shape[:2], (240, 320))

    def test_haar_falls_back_to_alt_and_forwards_detection_parameters(self):
        with tempfile.TemporaryDirectory() as folder:
            root = Path(folder)
            (root / 'haarcascade_frontalface_default.xml').touch()
            (root / 'haarcascade_frontalface_alt.xml').touch()
            detected = []

            def classifier(filename):
                if Path(filename).stem.endswith('_default'):
                    return types.SimpleNamespace(empty=lambda: True)
                def detect(gray, **options):
                    self.assertEqual(gray.shape, (100, 120))
                    detected.append(options)
                    return [(20, 20, 48, 48)]
                return types.SimpleNamespace(empty=lambda: False, detectMultiScale=detect)

            with (patch.object(cv2, 'CascadeClassifier', classifier, create=True),
                  patch.object(cv2, 'data', types.SimpleNamespace(haarcascades=folder), create=True)):
                frame = np.zeros((100, 120, 3), dtype=np.uint8)
                result, facts = STREAM.Algorithms(cv2, np).apply(
                    frame, STREAM.normalized({'algo': 6, 'faceNeighbors': 7, 'faceMinSize': 34}))
            self.assertEqual(dict(facts)['人脸'], '1')
            self.assertEqual(detected, [{'scaleFactor': 1.15, 'minNeighbors': 7, 'minSize': (34, 34)}])
            self.assertTrue(result[20, 20].any())

    def test_xwd_fallback_decodes_padded_truecolor_scanlines(self):
        fields = [100, 7, 2, 24, 2, 1, 0, 0, 32, 0, 32, 32, 8, 5,
                  0xff0000, 0xff00, 0xff, 8, 256, 0, 2, 1, 0, 0, 0]
        data = struct.pack('!25I', *fields) + bytes((0, 0, 255, 0, 0, 255, 0, 0))
        frame = STREAM.xwd_pixels(data, np)
        self.assertEqual(frame.tolist(), [[[0, 0, 255], [0, 255, 0]]])
        self.assertIsNone(STREAM.xwd_pixels(data[:-1], np))
