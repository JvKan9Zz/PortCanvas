"""Pull-based Vision Studio: one stdout JPEG per credit; line-delimited JSON on stderr."""
import select
import signal

# PortCanvas runs both sources in one Python process. Standalone use imports the inspector.
if 'holder_ids' not in globals():
    from vision_inspect import (
        SHARED_FRAME, VIDEO_PATH, display_environment, display_windows,
        holder_ids, shared_frame,
    )
    import json
    import os
    import re
    import shutil
    import struct
    import subprocess
    import sys
    import time

PRESETS = ((0, 10, 50, 255, 50, 255), (100, 124, 80, 255, 50, 255),
           (35, 85, 60, 255, 50, 255), (18, 34, 80, 255, 80, 255))
MAX_FRAME = 1024 * 1024
INPUT_LIMIT = 8192


def normalized(config):
    def integer(key, default, low, high):
        value = config.get(key, default)
        return min(high, max(low, value)) if type(value) is int else default

    def flag(key, default=False):
        value = config.get(key, default)
        return value if isinstance(value, bool) else default

    source = config.get('source')
    device, window = config.get('path'), config.get('windowId')
    result = {'source': source if source in ('auto', 'camera', 'watch') else 'auto',
              'path': device if isinstance(device, str) and VIDEO_PATH.fullmatch(device) else '',
              'windowId': window if isinstance(window, str) and re.fullmatch(r'0x[0-9a-fA-F]{1,16}|screen', window) else '',
              'fps': integer('fps', 15, 1, 30), 'algo': integer('algo', 0, 0, 6)}
    for key, default, low, high in (
        ('blur', 5, 1, 11), ('binaryMode', 2, 0, 2), ('thresh', 127, 0, 255),
        ('featureMode', 0, 0, 2), ('cannyLow', 30, 0, 255), ('cannyHigh', 100, 0, 255),
        ('hsvPreset', 0, 0, 4), ('hsvView', 0, 0, 2),
        ('hLow', 0, 0, 179), ('hHigh', 10, 0, 179), ('sLow', 50, 0, 255),
        ('sHigh', 255, 0, 255), ('vLow', 50, 0, 255), ('vHigh', 255, 0, 255),
        ('houghParam2', 35, 1, 200), ('minRadius', 12, 1, 160),
        ('tagFamily', 0, 0, 2), ('faceNeighbors', 4, 1, 20), ('faceMinSize', 36, 8, 256),
    ):
        result[key] = integer(key, default, low, high)
    result['blur'] |= 1
    for key in ('applyOnWatch', 'flipH', 'flipV', 'crosshair', 'morph', 'lineRoi',
                'drawContours', 'tagGuide'):
        result[key] = flag(key, key in ('crosshair', 'morph', 'drawContours', 'tagGuide'))
    return result


class Controls:
    def __init__(self, config):
        self.config = normalized(config)
        self.line = bytearray()
        self.credit = True
        self.closed = False
        self.last = time.monotonic()

    def feed(self, data):
        for byte in data:
            if self.line or byte == ord('{'):
                if byte == 10:
                    try:
                        packet = json.loads(self.line)
                        if isinstance(packet, dict) and packet.get('cmd') == 'config' and isinstance(packet.get('config'), dict):
                            self.config = normalized(packet['config'])
                            self.last = time.monotonic()
                    except (ValueError, UnicodeError):
                        pass
                    self.line.clear()
                else:
                    self.line.append(byte)
                    if len(self.line) > INPUT_LIMIT:
                        raise ValueError('视觉控制消息过长。')
            elif byte in (ord('.'), ord('r')):
                self.last = time.monotonic()
                if byte == ord('r'):
                    self.credit = True

    def receive(self, timeout):
        if select.select([0], [], [], timeout)[0]:
            data = os.read(0, INPUT_LIMIT)
            if data:
                self.feed(data)
            else:
                self.closed = True
        return not self.closed and time.monotonic() - self.last < 6


def choose_source(config, shared, owner_ids, windows):
    mode = config['source']
    if mode == 'camera' or (mode == 'auto' and not shared and not owner_ids):
        return ('camera', config['path'])
    if shared:
        return ('shared', SHARED_FRAME)
    requested = config['windowId']
    matches = [window for window in windows if window['id'] == requested]
    if not matches and not requested:
        matches = [window for window in windows
                   if re.search(r'opencv|cv2|camera|vision|frame|result|preview|detection|apriltag|canny|mask|摄像|视觉|检测',
                                window['title'], re.I)]
    chosen = matches[0] if matches else next((w for w in windows if w['id'] == 'screen'), None)
    return ('x11', chosen['id']) if chosen else ('waiting', '')


def xwd_pixels(data, np):
    if len(data) < 100:
        return None
    fields = struct.unpack('!25I', data[:100])
    header, version, layout, _, width, height, offset, byte_order = fields[:8]
    bpp, stride = fields[11:13]
    masks = fields[14:17]
    start = header + fields[19] * 12
    step = bpp // 8
    if (version != 7 or layout != 2 or offset or not (0 < width <= 4096 and 0 < height <= 4096)
            or step not in (2, 3, 4) or not all(masks) or stride < width * step
            or start < 100 or start + stride * height > len(data)):
        return None
    raw = np.frombuffer(data, dtype=np.uint8, count=stride * height, offset=start)
    raw = raw.reshape(height, stride)[:, :width * step].reshape(height, width, step)
    packed = np.zeros((height, width), dtype=np.uint32)
    for index in range(step):
        shift = 8 * (index if byte_order == 0 else step - index - 1)
        packed |= raw[:, :, index].astype(np.uint32) << shift
    channels = []
    for mask in reversed(masks):
        shift = (mask & -mask).bit_length() - 1
        channels.append((((packed & mask) >> shift) * 255 // (mask >> shift)).astype(np.uint8))
    return np.stack(channels, axis=-1)


class Capture:
    def __init__(self, cv2, np):
        self.cv2, self.np = cv2, np
        self.device = None
        self.child = None
        self.signature = None
        self.dimensions = None
        self.fallback = False

    def close(self):
        if self.device is not None:
            self.device.release()
            self.device = None
        self.close_child()
        self.signature = None

    def close_child(self):
        if self.child is not None:
            child, self.child = self.child, None
            try:
                child.terminate()
                child.wait(timeout=0.4)
            except subprocess.TimeoutExpired:
                child.kill()
                child.wait()
            finally:
                child.stdout.close()

    def frame(self, source, target, fps, env, geometry):
        signature = (source, target, fps if source == 'x11' else 0, geometry)
        if signature != self.signature:
            self.close()
            self.signature, self.fallback = signature, False
            if source == 'camera':
                self.device = self.cv2.VideoCapture(target, self.cv2.CAP_V4L2)
                if not self.device.isOpened():
                    raise RuntimeError('摄像头无法打开，可能正在使用。')
                for key, value in ((self.cv2.CAP_PROP_FOURCC, self.cv2.VideoWriter_fourcc(*'MJPG')),
                                   (self.cv2.CAP_PROP_FRAME_WIDTH, 640),
                                   (self.cv2.CAP_PROP_FRAME_HEIGHT, 480),
                                   (self.cv2.CAP_PROP_BUFFERSIZE, 1)):
                    self.device.set(key, value)
        if source == 'camera':
            ok, frame = self.device.read()
            return frame if ok else None
        if source == 'shared':
            if not shared_frame()['available']:
                return None
            try:
                with open(target, 'rb') as stream:
                    data = stream.read(MAX_FRAME + 1)
                if 0 < len(data) <= MAX_FRAME:
                    return self.cv2.imdecode(self.np.frombuffer(data, self.np.uint8), self.cv2.IMREAD_COLOR)
            except OSError:
                pass
            return None
        if source == 'x11' and geometry:
            return self.x11(target, fps, env, geometry)
        return None

    def x11(self, target, fps, env, geometry):
        x, y, width, height = geometry
        if not self.fallback and shutil.which('ffmpeg'):
            if self.child is None:
                scale = min(1., 960 / width, 720 / height)
                out_w, out_h = max(2, int(width * scale)), max(2, int(height * scale))
                self.dimensions = out_w, out_h
                try:
                    self.child = subprocess.Popen(
                        ['ffmpeg', '-hide_banner', '-loglevel', 'error', '-nostdin',
                         '-f', 'x11grab', '-draw_mouse', '0', '-framerate', str(fps),
                         '-video_size', '{}x{}'.format(width, height), '-i', '{}+{},{}'.format(env['DISPLAY'], x, y),
                         '-vf', 'scale={}:{}'.format(out_w, out_h), '-an',
                         '-threads', '1', '-pix_fmt', 'bgr24', '-f', 'rawvideo', 'pipe:1'],
                        stdin=subprocess.DEVNULL, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL, env=env)
                except OSError:
                    self.fallback = True
            if self.child is not None:
                out_w, out_h = self.dimensions
                size = out_w * out_h * 3
                payload = bytearray()
                until = time.monotonic() + 1.2
                while len(payload) < size:
                    wait = until - time.monotonic()
                    if wait <= 0 or not select.select([self.child.stdout], [], [], wait)[0]:
                        break
                    chunk = os.read(self.child.stdout.fileno(), size - len(payload))
                    if not chunk:
                        break
                    payload.extend(chunk)
                if len(payload) == size:
                    return self.np.frombuffer(payload, dtype=self.np.uint8).reshape(out_h, out_w, 3).copy()
                self.close_child()
                self.fallback = True
        if not shutil.which('xwd'):
            return None
        try:
            result = subprocess.run(['xwd', '-silent'] + (
                ['-root'] if target == 'screen' else ['-id', target]),
                stdout=subprocess.PIPE, stderr=subprocess.DEVNULL, env=env, timeout=0.8)
            if result.returncode == 0:
                return xwd_pixels(result.stdout, self.np)
        except (OSError, subprocess.TimeoutExpired):
            pass
        return None


class Algorithms:
    def __init__(self, cv2, np):
        self.cv, self.np = cv2, np
        self.tags = {}
        self.cascade = None

    def apply(self, frame, c):
        cv, np = self.cv, self.np
        h, w = frame.shape[:2]
        algo = c['algo']
        if algo == 0:
            if c['flipH'] or c['flipV']:
                frame = cv.flip(frame, -1 if c['flipH'] and c['flipV'] else int(c['flipH']))
            if c['crosshair']:
                cv.drawMarker(frame, (w // 2, h // 2), (40, 235, 180), markerSize=34, thickness=1)
            return frame, []
        if algo == 3:
            return self.hsv(frame, c)
        gray = cv.cvtColor(frame, cv.COLOR_BGR2GRAY)
        if algo == 1:
            gray = cv.GaussianBlur(gray, (c['blur'], c['blur']), 0)
            if c['binaryMode']:
                kind = cv.THRESH_BINARY | (cv.THRESH_OTSU if c['binaryMode'] == 2 else 0)
                threshold, gray = cv.threshold(gray, c['thresh'], 255, kind)
                return gray, [('阈值', '{:.0f}'.format(threshold))]
            return gray, []
        if algo == 2:
            if c['featureMode'] == 0:
                return cv.Canny(gray, *sorted((c['cannyLow'], c['cannyHigh']))), []
            if c['featureMode'] == 1:
                sx = cv.convertScaleAbs(cv.Sobel(gray, cv.CV_16S, 1, 0))
                sy = cv.convertScaleAbs(cv.Sobel(gray, cv.CV_16S, 0, 1))
                return cv.addWeighted(sx, .5, sy, .5, 0), []
            corners = cv.goodFeaturesToTrack(gray, 64, .03, 8)
            if corners is not None:
                for x, y in np.rint(corners.reshape(-1, 2)).astype(int):
                    cv.circle(frame, (x, y), 4, (45, 235, 180), 2)
            return frame, [('角点', str(len(corners) if corners is not None else 0))]
        if algo == 4:
            gray = cv.medianBlur(gray, 5)
            circles = cv.HoughCircles(gray, cv.HOUGH_GRADIENT, 1.2, 24,
                                      param1=100, param2=c['houghParam2'], minRadius=c['minRadius'])
            count = 0
            if circles is not None:
                for x, y, radius in np.rint(circles[0][:48]).astype(int):
                    cv.circle(frame, (x, y), radius, (45, 235, 180), 2)
                    count += 1
            shapes = 0
            if c['drawContours']:
                contours, _ = cv.findContours(cv.Canny(gray, 50, 150), cv.RETR_EXTERNAL, cv.CHAIN_APPROX_SIMPLE)
                for contour in contours:
                    if cv.contourArea(contour) >= 80:
                        polygon = cv.approxPolyDP(contour, .025 * cv.arcLength(contour, True), True)
                        cv.polylines(frame, [polygon], True, (60, 180, 240), 1)
                        shapes += 1
            return frame, [('圆形', str(count)), ('轮廓', str(shapes))]
        if algo == 5:
            return self.april(frame, gray, c)
        if self.cascade is None:
            classifier = getattr(cv, 'CascadeClassifier', None)
            path = getattr(getattr(cv, 'data', None), 'haarcascades', '')
            for filename in ('haarcascade_frontalface_default.xml', 'haarcascade_frontalface_alt.xml'):
                candidate = os.path.join(path, filename)
                if classifier and path and os.path.isfile(candidate):
                    cascade = classifier(candidate)
                    if not cascade.empty():
                        self.cascade = cascade
                        break
            if self.cascade is None:
                return frame, [('状态', '缺少 Haar 人脸分类器')]
        faces = self.cascade.detectMultiScale(cv.equalizeHist(gray), scaleFactor=1.15,
                                              minNeighbors=c['faceNeighbors'], minSize=(c['faceMinSize'],) * 2)
        for x, y, width, height in faces:
            cv.rectangle(frame, (x, y), (x + width, y + height), (45, 235, 180), 2)
        return frame, [('人脸', str(len(faces)))]

    def hsv(self, frame, c):
        cv = self.cv
        h, w = frame.shape[:2]
        offset = h * 4 // 5 if c['lineRoi'] else 0
        area = frame[offset:]
        hsv = cv.cvtColor(area, cv.COLOR_BGR2HSV)
        preset = c['hsvPreset']
        if preset < len(PRESETS):
            hl, hh, sl, sh, vl, vh = PRESETS[preset]
        else:
            hl, hh = sorted((c['hLow'], c['hHigh']))
            sl, sh = sorted((c['sLow'], c['sHigh']))
            vl, vh = sorted((c['vLow'], c['vHigh']))
        mask = cv.inRange(hsv, (hl, sl, vl), (hh, sh, vh))
        if preset == 0:
            mask |= cv.inRange(hsv, (170, sl, vl), (179, sh, vh))
        if c['morph']:
            kernel = cv.getStructuringElement(cv.MORPH_ELLIPSE, (3, 3))
            mask = cv.medianBlur(cv.dilate(cv.erode(mask, kernel), kernel), 5)
        contours, _ = cv.findContours(mask, cv.RETR_EXTERNAL, cv.CHAIN_APPROX_SIMPLE)
        contour = max(contours, key=cv.contourArea, default=None)
        details = []
        if c['hsvView'] == 1:
            output = self.np.zeros((h, w), dtype=self.np.uint8)
            output[offset:] = mask
        elif c['hsvView'] == 2:
            output = self.np.zeros_like(frame)
            output[offset:] = cv.bitwise_and(area, area, mask=mask)
        else:
            output = frame
            if offset:
                cv.line(output, (0, offset), (w - 1, offset), (60, 200, 255), 1)
        if contour is not None:
            m = cv.moments(contour)
            if m['m00'] > 0:
                cx, cy = int(m['m10'] / m['m00']), int(m['m01'] / m['m00']) + offset
                x, y, width, height = cv.boundingRect(contour)
                if c['hsvView'] == 0:
                    cv.rectangle(output, (x, y + offset), (x + width, y + height + offset), (45, 235, 180), 2)
                    cv.circle(output, (cx, cy), 4, (45, 235, 180), -1)
                details = [('质心', '{}, {}'.format(cx, cy)), ('面积', '{:.0f} px²'.format(m['m00'])),
                           ('偏移', '{:+.0f} px'.format(cx - w / 2))]
        return output, details

    def april(self, frame, gray, c):
        cv, np = self.cv, self.np
        family = c['tagFamily']
        if family not in self.tags:
            try:
                import apriltag
            except ImportError:
                return frame, [('状态', '板端缺少 apriltag')]
            families = (('tag36h11',), ('tag25h9',), ('tag36h11', 'tag25h9'))[family]
            self.tags[family] = [apriltag.Detector(apriltag.DetectorOptions(families=name))
                                 for name in families]
        detections = [d for detector in self.tags[family] for d in detector.detect(gray)][:24]
        for detection in detections:
            center = tuple(np.rint(detection.center).astype(int))
            cv.polylines(frame, [np.rint(detection.corners).astype(np.int32)], True, (45, 235, 180), 2)
            cv.circle(frame, center, 3, (45, 235, 180), -1)
            cv.putText(frame, 'ID {}'.format(detection.tag_id), center, cv.FONT_HERSHEY_SIMPLEX, .5, (45, 235, 180), 1)
        if c['tagGuide']:
            h, w = gray.shape
            for offset in (-40, 40):
                x = w // 2 + offset
                cv.line(frame, (x, 0), (x, h - 1), (60, 200, 255), 1)
        return frame, [('标签', str(len(detections))),
                       ('标签 ID', ', '.join(str(d.tag_id) for d in detections) or '—')]


def frame_geometry(target, windows):
    screen = next((window for window in windows if window['id'] == 'screen'), None)
    box = next((window for window in windows if window['id'] == target), None)
    if not screen or not box:
        return None
    x, y = max(0, box['x']), max(0, box['y'])
    width = min(screen['width'], box['x'] + box['width']) - x
    height = min(screen['height'], box['y'] + box['height']) - y
    return (x, y, width, height) if width > 0 and height > 0 else None


def telemetry(source, label, fps, ms, frame, details, summary):
    height, width = frame.shape[:2] if frame is not None else (0, 0)
    print('PORTCANVAS_VISION:' + json.dumps(
        {'pid': os.getpid(), 'activeSource': source, 'sourceLabel': label, 'fps': round(fps, 1),
         'procMs': round(ms, 1), 'width': width, 'height': height, 'summary': summary,
         'details': [{'label': key, 'value': value} for key, value in details]},
        ensure_ascii=False), file=sys.stderr, flush=True)


def run(config):
    os.environ.setdefault('OPENCV_VIDEOIO_V4L_SELECT_TIMEOUT', '2')
    import cv2
    import numpy as np

    cv2.setNumThreads(1)
    capture, algorithms = Capture(cv2, np), Algorithms(cv2, np)
    control = Controls(config)
    env = display_environment()
    windows, owners = [], set()
    route_key, next_probe, next_frame, next_telemetry = None, 0., 0., 0.
    fps_started, fps_count, measured_fps = time.monotonic(), 0, 0.
    selected_geometry, source, target = None, 'waiting', ''
    camera_retry_at = 0.
    try:
        while control.receive(0.03):
            now = time.monotonic()
            c = control.config
            key = c['source'], c['path'], c['windowId']
            if now >= next_probe or key != route_key:
                if key != route_key:
                    camera_retry_at = 0.
                camera_failed = now < camera_retry_at
                route_key, next_probe = key, now + 2.5
                owners = holder_ids([c['path']], ignore_pid=os.getpid()).get(c['path'], set()) if c['path'] and c['source'] != 'watch' else set()
                if c['source'] == 'camera' and owners:
                    raise RuntimeError('摄像头被板端程序占用，请使用伴随监视。')
                if c['source'] == 'camera' and not c['path']:
                    raise RuntimeError('请选择有效的视频设备。')
                shared = c['source'] != 'camera' and shared_frame()['available']
                if not shared and (c['source'] == 'watch' or owners or not c['path'] or camera_failed):
                    windows = display_windows(env)
                if c['source'] == 'auto' and (not c['path'] or camera_failed) and not shared:
                    source, target = choose_source(dict(c, source='watch'), False, set(), windows)
                else:
                    source, target = choose_source(c, shared, owners, windows)
                selected_geometry = frame_geometry(target, windows) if source == 'x11' else None
                if source == 'x11' and not selected_geometry and target != 'screen':
                    target = 'screen'
                    selected_geometry = frame_geometry(target, windows)
            if not control.credit or now < next_frame:
                continue
            t0 = time.monotonic()
            try:
                frame = capture.frame(source, target, c['fps'], env, selected_geometry)
            except RuntimeError:
                if c['source'] != 'auto' or source != 'camera':
                    raise
                capture.close()
                camera_retry_at, next_probe = now + 10., 0.
                continue
            if source == 'shared' and frame is None and not shared_frame()['available']:
                next_probe = 0.
            started_processing = time.monotonic()
            details = []
            label = {'camera': '直采', 'shared': '共享帧', 'x11': (
                '整屏' if target == 'screen' else 'X11 窗口'), 'waiting': '伴随监视'}[source]
            summary = '等待板端画面'
            if frame is not None:
                h, w = frame.shape[:2]
                scale = min(1., 960 / w, 720 / h)
                if scale < 1.:
                    frame = cv2.resize(frame, (max(1, int(w * scale)), max(1, int(h * scale))))
                if source == 'camera' or c['applyOnWatch']:
                    frame, details = algorithms.apply(frame, c)
                ok, jpeg = cv2.imencode('.jpg', frame, (cv2.IMWRITE_JPEG_QUALITY, 78))
                if ok and 0 < jpeg.size <= MAX_FRAME:
                    sys.stdout.buffer.write(struct.pack('!I', jpeg.size))
                    sys.stdout.buffer.write(jpeg.tobytes())
                    sys.stdout.buffer.flush()
                    control.credit = False
                    fps_count += 1
                    summary = '实时画面'
            now = time.monotonic()
            if now - fps_started >= 1.:
                measured_fps, fps_count, fps_started = fps_count / (now - fps_started), 0, now
            if now >= next_telemetry:
                telemetry(source, label, measured_fps, (now - started_processing) * 1000, frame, details, summary)
                next_telemetry = now + .2
            next_frame = max(t0 + 1 / c['fps'], now)
    finally:
        capture.close()


def end_stream(signum, frame):
    raise SystemExit(0)


if __name__ == '__main__':
    signal.signal(signal.SIGHUP, end_stream)
    signal.signal(signal.SIGTERM, end_stream)
    try:
        run(json.loads(sys.argv[1]) if len(sys.argv) > 1 else {})
    except (ValueError, RuntimeError, OSError, ImportError) as error:
        print(str(error), file=sys.stderr, flush=True)
        sys.exit(1)
