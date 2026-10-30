"""One SSH channel owns one shared-X11 server. No daemon or system changes."""
import json
import os
import signal
import subprocess
import sys
import threading
import shutil
import sysconfig
import ctypes
import ctypes.util
from fractions import Fraction
from pathlib import Path

profile = json.loads(sys.argv[1])
runtime = Path.home() / 'robot-research/wheel-combat-studio/v0.1/runtime'
system_binary = shutil.which('x11vnc')
binary = Path(system_binary) if system_binary else runtime / 'usr/bin/x11vnc'
if not binary.is_file():
    sys.exit('未找到 x11vnc。当前后端需要已登录的 Linux/X11 桌面及 x11vnc；请按支持范围准备组件。')
auth = profile.get('xauthority') or str(Path('/run/user') / str(os.getuid()) / 'gdm/Xauthority')
if not Path(auth).is_file():
    auth = str(Path.home() / '.Xauthority')
if not Path(auth).is_file():
    sys.exit('找不到桌面授权文件，请确认该用户已经登录 Linux 桌面。')
env = os.environ.copy()
if not system_binary:
    multiarch = sysconfig.get_config_var('MULTIARCH')
    library = runtime / 'usr/lib' / str(multiarch)
    if library.is_dir():
        env['LD_LIBRARY_PATH'] = str(library) + (':' + env['LD_LIBRARY_PATH'] if env.get('LD_LIBRARY_PATH') else '')
parent_pid = os.getpid()

def scale_options(display_name, auth, width_limit, height_limit):
    # Scale the transmitted framebuffer; never resize the user's X11 display.
    display = None
    try:
        library = ctypes.util.find_library('X11')
        if not library:
            return []
        os.environ['XAUTHORITY'] = auth
        x11 = ctypes.CDLL(library)
        x11.XOpenDisplay.argtypes = [ctypes.c_char_p]
        x11.XOpenDisplay.restype = ctypes.c_void_p
        x11.XCloseDisplay.argtypes = [ctypes.c_void_p]
        display = x11.XOpenDisplay(display_name.encode())
        if not display:
            return []
        x11.XDefaultScreen.argtypes = [ctypes.c_void_p]
        screen = x11.XDefaultScreen(display)
        for name in ['XDisplayWidth', 'XDisplayHeight']:
            getattr(x11, name).argtypes = [ctypes.c_void_p, ctypes.c_int]
        width, height = x11.XDisplayWidth(display, screen), x11.XDisplayHeight(display, screen)
        if width <= 0 or height <= 0:
            return []
        scale = min(Fraction(1), Fraction(width_limit, width), Fraction(height_limit, height))
        return ['-scale', f'{scale.numerator}/{scale.denominator}:nb,nocr'] if scale < 1 else []
    except (OSError, AttributeError):
        return []  # The existing native-resolution VNC path remains available.
    finally:
        if display:
            x11.XCloseDisplay(display)
tuning = profile.get('tuning', {})
transport_options = []
if tuning:
    wait, defer = tuning['wait'], tuning['defer']
    assert type(wait) is int and 1 <= wait <= 1000
    assert type(defer) is int and 0 <= defer <= 100
    assert 0 < tuning['frameInterval'] <= 1
    # Avoid x11vnc's synchronous startup bandwidth probe, which can suspend
    # scanning/input delivery for 5–15 seconds after a large window redraw.
    transport_options = ['-speeds', 'lan', '-wait', str(wait), '-defer', str(defer), '-setdefer', '-1', '-input_eagerly', '-nowf', '-noscr', '-slow_fb', str(tuning['frameInterval'])]
    width_limit, height_limit = tuning['maxWidth'], tuning['maxHeight']
    if width_limit and height_limit:
        transport_options.extend(scale_options(profile['display'], auth, width_limit, height_limit))
def child_setup():
    if sys.platform.startswith('linux'):
        libc = ctypes.CDLL(None, use_errno=True)
        if libc.prctl(1, signal.SIGKILL, 0, 0, 0) != 0:
            raise OSError(ctypes.get_errno(), 'Unable to bind desktop process lifetime')
        if os.getppid() != parent_pid:
            os._exit(1)

process = subprocess.Popen([
    str(binary), '-display', profile['display'], '-auth', auth,
    '-localhost', '-no6', '-autoport', '5900', '-nopw', '-forever',
    '-shared', '-quiet', '-noipv6', *transport_options
], env=env, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True, bufsize=1, preexec_fn=child_setup)

stopping = threading.Event()

def stop(*_):
    if stopping.is_set():
        return
    stopping.set()
    if process.poll() is None:
        process.terminate()
        try:
            process.wait(timeout=2)
        except subprocess.TimeoutExpired:
            process.kill()
            process.wait(timeout=2)

def watch_channel():
    try:
        while sys.stdin.buffer.read(1):
            pass
    finally:
        stop()

signal.signal(signal.SIGTERM, stop)
signal.signal(signal.SIGHUP, stop)
threading.Thread(target=watch_channel, daemon=True).start()
try:
    for line in process.stdout:
        if line.startswith('PORT='):
            print('STUDIO_DESKTOP=' + json.dumps({'port': int(line.split('=')[1]), 'display': profile['display']}), flush=True)
        else:
            print(line.rstrip(), file=sys.stderr, flush=True)
    code = process.wait()
    sys.exit(0 if stopping.is_set() else code)
finally:
    stop()
