"""One SSH channel owns one shared-X11 server. No daemon or system changes."""
import json
import os
import signal
import subprocess
import sys
import threading
from pathlib import Path

profile = json.loads(sys.argv[1])
runtime = Path.home() / 'robot-research/wheel-combat-studio/v0.1/runtime'
binary = runtime / 'usr/bin/x11vnc'
if not binary.is_file():
    sys.exit('桌面组件未准备，请先运行交付说明中的板端准备步骤。')
auth = profile.get('xauthority') or str(Path('/run/user') / str(os.getuid()) / 'gdm/Xauthority')
if not Path(auth).is_file():
    auth = str(Path.home() / '.Xauthority')
if not Path(auth).is_file():
    sys.exit('找不到桌面授权文件，请确认该用户已经登录 Linux 桌面。')
env = os.environ.copy()
env['LD_LIBRARY_PATH'] = str(runtime / 'usr/lib/aarch64-linux-gnu')
process = subprocess.Popen([
    str(binary), '-display', profile['display'], '-auth', auth,
    '-localhost', '-no6', '-autoport', '5900', '-nopw', '-forever',
    '-shared', '-quiet', '-noipv6'
], env=env, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True, bufsize=1)

stopping = threading.Event()

def stop(*_):
    stopping.set()
    if process.poll() is None:
        process.terminate()

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
