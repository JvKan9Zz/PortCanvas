import json
import os
import re
import time
from pathlib import Path


def read(path):
    try:
        return Path(path).read_text(encoding='utf-8', errors='replace')
    except OSError:
        return ''


def cpu_ticks(line):
    ticks = [int(value) for value in line.split()[1:9]]
    return sum(ticks), ticks[3] + ticks[4]


def cpu_frequency():
    values = []
    for path in Path('/sys/devices/system/cpu/cpufreq').glob('policy*/scaling_cur_freq'):
        value = read(path).strip()
        if value.isdecimal() and int(value) > 0:
            values.append(int(value) / 1000)
    if values:
        return max(values)
    for line in read('/proc/cpuinfo').splitlines():
        key, _, value = line.partition(':')
        if key.strip() == 'cpu MHz':
            try:
                mhz = float(value)
                if 0 < mhz < 100_000:
                    return mhz
            except ValueError:
                pass
    return None


def mount_field(value):
    return re.sub(r'\\([0-7]{3})', lambda match: chr(int(match[1], 8)), value)


def storage_rows(mounts, statvfs=None):
    if statvfs is None:
        statvfs = os.statvfs
    priority = {'/': 0, '/home': 1, '/boot': 2}
    candidates = []
    for line in mounts.splitlines():
        fields = line.split()
        if len(fields) < 3:
            continue
        device, mount, fs_type = map(mount_field, fields[:3])
        if mount != '/' and not device.startswith('/dev/'):
            continue
        if device.startswith(('/dev/loop', '/dev/ram', '/dev/zram')):
            continue
        external = mount.startswith(('/media/', '/run/media/', '/mnt/'))
        candidates.append((priority.get(mount, 3 if external else 4), mount, device, fs_type))

    rows = []
    devices = set()
    for _, mount, device, fs_type in sorted(candidates):
        identity = os.path.realpath(device) if device.startswith('/dev/') else device
        if identity in devices:
            continue
        try:
            info = statvfs(mount)
        except OSError:
            continue
        total = info.f_blocks * info.f_frsize
        if total <= 0:
            continue
        devices.add(identity)
        rows.append({'mount': mount, 'device': device, 'fsType': fs_type,
                     'totalBytes': total, 'usedBytes': (info.f_blocks - info.f_bfree) * info.f_frsize,
                     'availableBytes': max(0, info.f_bavail * info.f_frsize)})
        if len(rows) == 6:
            break
    return rows


def thermal_rows(root):
    rows = []
    for zone in sorted(Path(root).glob('thermal_zone*')):
        try:
            celsius = int(read(zone / 'temp')) / 1000
        except ValueError:
            continue
        if -40 <= celsius <= 150:
            rows.append({'label': read(zone / 'type').strip() or zone.name, 'celsius': celsius})
    return rows


def system_facts():
    release = read('/etc/os-release') or read('/usr/lib/os-release')
    os_name = next((line.partition('=')[2].strip().strip('"\'') for line in release.splitlines()
                    if line.startswith('PRETTY_NAME=')), None)
    host = os.uname()
    return {'hostname': host.nodename or None, 'os': os_name,
            'kernel': host.release or None, 'arch': host.machine or None}


def collect():
    clock = time.monotonic()
    cpu_lines = Path('/proc/stat').read_text().splitlines()
    cpu_total, cpu_idle = cpu_ticks(cpu_lines[0])
    cores = []
    for line in cpu_lines[1:]:
        label = line.split(maxsplit=1)[0]
        if not label.startswith('cpu') or not label[3:].isdecimal():
            break
        total, idle = cpu_ticks(line)
        cores.append({'id': int(label[3:]), 'total': total, 'idle': idle})
    memory = {line.split(':')[0]: int(line.split()[1]) * 1024
              for line in Path('/proc/meminfo').read_text().splitlines() if ':' in line}
    network = []
    for line in Path('/proc/net/dev').read_text().splitlines()[2:]:
        name, values = line.split(':', 1)
        name = name.strip()
        if name != 'lo':
            numbers = values.split()
            network.append({'name': name, 'received': int(numbers[0]), 'sent': int(numbers[8])})
    return {'clock': clock, 'cpuTotal': cpu_total, 'cpuIdle': cpu_idle,
            'cores': os.cpu_count(), 'cpuCores': cores, 'cpuFreqMhz': cpu_frequency(),
            'memoryTotal': memory['MemTotal'], 'memoryAvailable': memory['MemAvailable'],
            'memoryCached': memory.get('Cached'), 'swapTotal': memory['SwapTotal'],
            'swapFree': memory['SwapFree'], 'uptime': float(Path('/proc/uptime').read_text().split()[0]),
            'load': os.getloadavg(), 'network': network, 'storage': storage_rows(read('/proc/mounts')),
            'thermals': thermal_rows('/sys/class/thermal'), 'system': system_facts()}


if __name__ == '__main__':
    print(json.dumps(collect()))
