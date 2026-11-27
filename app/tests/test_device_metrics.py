"""Exercise Linux metrics formatting with synthetic values and no device access."""
import importlib.util
import tempfile
import unittest
from pathlib import Path
from types import SimpleNamespace
from unittest.mock import patch

SOURCE = Path(__file__).resolve().parents[1] / 'core/resources/device_metrics.py'
SPEC = importlib.util.spec_from_file_location('device_metrics', SOURCE)
METRICS = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(METRICS)


class DeviceMetricsTests(unittest.TestCase):
    def test_old_fields_and_per_cpu_ticks(self):
        contents = {
            '/proc/stat': 'cpu 10 0 20 60 10 0 0 0 99 99\ncpu0 5 0 10 30 5 0 0 0\nintr 0\n',
            '/proc/meminfo': 'MemTotal: 100 kB\nMemAvailable: 75 kB\nCached: 10 kB\nSwapTotal: 5 kB\nSwapFree: 2 kB\n',
            '/proc/net/dev': 'header\nheader\n lo: 1 0 0 0 0 0 0 0 2\neth0: 3 0 0 0 0 0 0 0 4',
            '/proc/uptime': '42.1 4.2',
        }
        with (patch.object(Path, 'read_text', lambda path, **opts: contents[path.as_posix()]),
              patch.object(METRICS, 'storage_rows', return_value=[]),
              patch.object(METRICS, 'read', return_value=''),
              patch.object(METRICS, 'cpu_frequency', return_value=None),
              patch.object(METRICS, 'system_facts', return_value={'hostname': 'fixture'}),
              patch.object(METRICS, 'thermal_rows', return_value=[]),
              patch.object(METRICS.os, 'getloadavg', return_value=(1., 2., 3.), create=True)):
            result = METRICS.collect()
        self.assertEqual((result['cpuTotal'], result['cpuIdle']), (100, 70))
        self.assertEqual(result['cpuCores'], [{'id': 0, 'total': 50, 'idle': 35}])
        self.assertEqual(result['memoryAvailable'], 75 * 1024)
        self.assertEqual(result['memoryCached'], 10 * 1024)
        self.assertEqual(result['network'], [{'name': 'eth0', 'received': 3, 'sent': 4}])
        self.assertEqual(result['uptime'], 42.1)
        self.assertIsNone(result['cpuFreqMhz'])

    def test_mounts_prefer_essential_and_external_and_deduplicate_devices(self):
        mounts = r'''proc /proc proc rw
tmpfs /run tmpfs rw
/dev/root / ext4 rw
/dev/alias /home ext4 rw
/dev/sda1 /boot vfat rw
/dev/sda2 /media/My\040USB exfat rw
/dev/loop1 /snap/foo squashfs ro
/dev/sda3 /gone ext4 rw'''
        info = SimpleNamespace(f_blocks=100, f_bfree=25, f_bavail=20, f_frsize=4096)

        def stat(mount):
            if mount == '/gone':
                raise FileNotFoundError
            return info

        with patch.object(METRICS.os.path, 'realpath', lambda path: '/dev/root' if path == '/dev/alias' else path):
            rows = METRICS.storage_rows(mounts, stat)
        self.assertEqual([row['mount'] for row in rows], ['/', '/boot', '/media/My USB'])
        self.assertEqual(rows[0]['totalBytes'], 409600)
        self.assertEqual(rows[0]['usedBytes'], 307200)
        self.assertEqual(rows[0]['availableBytes'], 81920)

    def test_only_valid_thermal_zones_are_reported(self):
        with tempfile.TemporaryDirectory() as root:
            for name, temp in [('thermal_zone0', '48500'), ('thermal_zone1', '999999'),
                               ('thermal_zone2', 'unavailable')]:
                zone = Path(root) / name
                zone.mkdir()
                (zone / 'temp').write_text(temp, encoding='utf-8')
            (Path(root) / 'thermal_zone0/type').write_text('cpu-thermal', encoding='utf-8')
            self.assertEqual(METRICS.thermal_rows(root), [{'label': 'cpu-thermal', 'celsius': 48.5}])
