"""Read board camera ownership and display sources without starting a capture."""
import errno
import importlib.util
import json
import os
import re
import shutil
import struct
import stat
import subprocess
import sys
import time
from pathlib import Path

SHARED_FRAME = '/dev/shm/portcanvas_cam.jpg'
VIDEO_PATH = re.compile(r'/dev/video[0-9]{1,3}')
GEOMETRY = re.compile(r'(\d+)x(\d+)[+-]\d+[+-]\d+\s+([+-]\d+)([+-]\d+)\s*$')
WINDOW = re.compile(r'^(\s*)(0x[0-9a-fA-F]+)\s+"([^"]+)"')


def command_output(args, env=None, timeout=0.8):
    try:
        result = subprocess.run(args, env=env, stdout=subprocess.PIPE,
                                stderr=subprocess.DEVNULL, timeout=timeout)
        if result.returncode == 0:
            return result.stdout.decode('utf-8', errors='replace')
    except (OSError, subprocess.TimeoutExpired):
        pass
    return ''


def display_environment():
    env = os.environ.copy()
    display = env.get('DISPLAY', '')
    env['DISPLAY'] = display if re.fullmatch(r':[0-9]+(?:\.[0-9]+)?', display) else ':0'
    if not env.get('XAUTHORITY'):
        for candidate in (Path('/run/user') / str(os.getuid()) / 'gdm/Xauthority',
                          Path.home() / '.Xauthority'):
            if candidate.is_file():
                env['XAUTHORITY'] = str(candidate)
                break
    return env


def window_details(window_id, env, timeout=0.8):
    output = command_output(['xwininfo', '-id', window_id, '-stats'] if window_id != 'screen'
                            else ['xwininfo', '-root', '-stats'], env, timeout)
    if not output or (window_id != 'screen' and 'Map State: IsViewable' not in output):
        return None

    def field(name):
        match = re.search(r'^\s*' + name + r':\s*(-?\d+)', output, re.M)
        return int(match[1]) if match else 0

    width, height = field('Width'), field('Height')
    if width <= 0 or height <= 0:
        return None
    return {'width': width, 'height': height, 'x': field('Absolute upper-left X'),
            'y': field('Absolute upper-left Y')}


def parse_windows(tree):
    rows = []
    ancestors = []
    for line in tree.splitlines():
        depth = len(line) - len(line.lstrip())
        if line.lstrip().startswith('0x'):
            while ancestors and depth <= ancestors[-1]:
                ancestors.pop()
        match, geometry = WINDOW.match(line), GEOMETRY.search(line)
        if not match or not geometry or ancestors:
            continue
        width, height, x, y = map(int, geometry.groups())
        if width < 32 or height < 24:
            continue
        title = match[3][:160]
        if title.lower() in ('mutter guard window', 'gnome-shell', 'desktop'):
            continue
        ancestors.append(depth)
        rows.append({'id': match[2], 'title': title, 'width': width,
                     'height': height, 'x': x, 'y': y})
        if len(rows) >= 40:
            break
    return rows


def display_windows(env):
    if not shutil.which('xwininfo'):
        return []
    screen = window_details('screen', env)
    if not screen and env['DISPLAY'] != ':0':
        env['DISPLAY'] = ':0'
        screen = window_details('screen', env)
    if not screen:
        return []
    rows = [{'id': 'screen', 'title': '整屏 (' + env['DISPLAY'] + ')', **screen}]
    candidates = parse_windows(command_output(['xwininfo', '-root', '-tree'], env))
    deadline = time.monotonic() + 0.6
    for candidate in candidates:
        remaining = deadline - time.monotonic()
        if remaining <= 0:
            break
        geometry = window_details(candidate['id'], env, min(remaining, 0.15))
        if geometry:
            rows.append({**candidate, **geometry})
    return rows


def shared_frame(path=SHARED_FRAME):
    try:
        info = os.stat(path)
        age_ms = round((time.time() - info.st_mtime) * 1000)
        return {'available': stat.S_ISREG(info.st_mode) and 0 <= age_ms < 3000,
                'ageMs': max(0, age_ms), 'path': path}
    except OSError:
        return {'available': False, 'ageMs': None, 'path': path}


def holder_ids(devices, proc=Path('/proc'), ignore_pid=None, fallback=True):
    by_device = {device: set() for device in devices}
    if not by_device:
        return by_device
    for process in proc.iterdir():
        if not process.name.isdecimal() or int(process.name) == ignore_pid:
            continue
        try:
            for fd in (process / 'fd').iterdir():
                try:
                    target = os.readlink(fd)
                    if target in by_device:
                        by_device[target].add(int(process.name))
                except OSError:
                    continue
        except (OSError, PermissionError):
            continue
    missing = [device for device, pids in by_device.items() if not pids]
    tools = (['fuser'] if shutil.which('fuser') else
             ['lsof', '-t'] if shutil.which('lsof') else []) if fallback and missing else []
    if tools:
        # Keep each PID list associated with the device it was queried for.
        for device in missing[:8]:
            result = command_output(tools + [device], timeout=0.15)
            by_device[device].update(int(pid) for pid in result.split()
                                     if pid.isdecimal() and int(pid) != ignore_pid)
    return by_device


def process_stats(process):
    record = (process / 'stat').read_text()
    fields = record[record.rfind(')') + 2:].split()
    return int(fields[11]) + int(fields[12]), int(fields[19]), int(fields[21])


def sanitized_command(raw):
    args = raw.split(b'\0')
    shown = []
    hide_next = False
    for arg in args:
        value = arg.decode('utf-8', errors='replace')
        if not value:
            continue
        if hide_next:
            shown.append('…')
            hide_next = False
            continue
        key, equal, _ = value.partition('=')
        if re.search(r'password|passwd|token|secret|api[-_]?key|credential', key, re.I) and (
                key.startswith('-') or equal):
            shown.append(key + ('=…' if equal else ''))
            hide_next = not equal
            continue
        value = re.sub(r'(\w+://)[^/@\s]+:[^/@\s]+@', r'\1…@', value)
        shown.append(value)
    return ' '.join(shown)[:320]


def holder_rows(owners, proc=Path('/proc')):
    pids = sorted({pid for owners_of_device in owners.values() for pid in owners_of_device})[:48]
    if not pids:
        return []
    starts = {}
    elapsed_at = time.monotonic()
    for pid in pids:
        try:
            starts[pid] = process_stats(proc / str(pid))
        except (OSError, IndexError, ValueError):
            continue
    if starts:
        time.sleep(0.075)
    sample_seconds = time.monotonic() - elapsed_at
    try:
        board_uptime = float((proc / 'uptime').read_text().split()[0])
    except (OSError, ValueError, IndexError):
        board_uptime = None
    ticks = os.sysconf('SC_CLK_TCK')
    page_size = os.sysconf('SC_PAGE_SIZE')
    rows = []
    for pid in pids:
        process = proc / str(pid)
        try:
            cpu, boot_ticks, rss = process_stats(process)
            previous = starts.get(pid)
            if previous and previous[1] != boot_ticks:
                continue
        except (OSError, IndexError, ValueError):
            continue
        try:
            name = (process / 'comm').read_text(errors='replace').strip()[:80]
        except OSError:
            name = ''
        try:
            with (process / 'cmdline').open('rb') as cmd:
                command = sanitized_command(cmd.read(4096))
        except OSError:
            command = ''
        for device, matches in owners.items():
            if pid in matches:
                rows.append({'device': device, 'pid': pid, 'name': name, 'cmdline': command,
                             'cpuPercent': None if previous is None else round(
                                 max(0, cpu - previous[0]) / ticks / max(sample_seconds, 0.001) * 100, 1),
                             'memoryMb': round(max(0, rss) * page_size / 1048576, 1),
                             'uptimeSec': None if board_uptime is None else max(0, board_uptime - boot_ticks / ticks)})
    return rows


def camera_capability(path):
    # Query the kernel, never request buffers or start streaming during inspection.
    try:
        import fcntl
        fd = os.open(path, os.O_RDONLY | os.O_NONBLOCK)
        try:
            cap = bytearray(104)
            fcntl.ioctl(fd, 0x80685600, cap, True)
            flags, device_flags = struct.unpack_from('=II', cap, 84)
            return bool((device_flags if flags & 0x80000000 else flags) & 0x1001), False
        finally:
            os.close(fd)
    except OSError as error:
        return error.errno != errno.ENOTTY, error.errno == errno.EBUSY


def inspect_board(ignore_pid=None):
    devices = sorted(path for path in Path('/dev').glob('video*') if VIDEO_PATH.fullmatch(str(path)))[:24]
    owners = holder_ids([str(path) for path in devices], ignore_pid=ignore_pid)
    holders = holder_rows(owners)
    cameras = []
    for device in devices:
        path = str(device)
        pids = sorted(owners[path])
        usable = os.access(path, os.R_OK | os.W_OK)
        capture, busy = (True, bool(pids)) if pids or not usable else camera_capability(path)
        if not capture:
            continue
        try:
            name = (Path('/sys/class/video4linux') / device.name / 'name').read_text().strip()
        except OSError:
            name = device.name
        cameras.append({'path': path, 'name': name[:100], 'accessible': usable,
                        'busy': busy, 'holders': pids})
    windows = display_windows(display_environment())
    cv2_spec = importlib.util.find_spec('cv2')
    haar = bool(cv2_spec and any(
        any((Path(root) / 'data' / name).is_file() for name in (
            'haarcascade_frontalface_default.xml', 'haarcascade_frontalface_alt.xml'))
        for root in (cv2_spec.submodule_search_locations or [])))
    return {'cameras': cameras, 'holders': holders, 'windows': windows, 'sharedFrame': shared_frame(),
            'capabilities': {'cv2': cv2_spec is not None, 'apriltag': importlib.util.find_spec('apriltag') is not None,
                             'haar': haar, 'ffmpeg': bool(shutil.which('ffmpeg')), 'x11': bool(windows)}}


if __name__ == '__main__' and sys.argv[0] != 'vision-stream':
    pid = sys.argv[1] if len(sys.argv) > 1 else ''
    print(json.dumps(inspect_board(int(pid) if pid.isdecimal() else None), ensure_ascii=False))
