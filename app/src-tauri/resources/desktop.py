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
    '-shared', '-quiet', '-noipv6'
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
