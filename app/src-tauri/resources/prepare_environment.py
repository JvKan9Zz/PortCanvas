"""Prepare missing feature dependencies in this SSH user's private cache only."""
import contextlib
import hashlib
import queue
import json
import os
import platform
import re
import select
import shlex
import shutil
import signal
import subprocess
import sys
import tempfile
import threading
import time
import uuid
from pathlib import Path

SCHEMA = 'portcanvas.runtime/1'
PACKAGES = {'desktop': ['x11vnc'], 'terminal': ['tmux', 'bash', 'coreutils'],
            'files': ['openssh-sftp-server'], 'camera': ['ffmpeg']}
stopping = threading.Event()
process_lock = threading.RLock()
active_process = None

def stop_process():
    with process_lock:
        process = active_process
    if process is not None and process.poll() is None:
        try:
            os.killpg(process.pid, signal.SIGTERM)
            process.wait(timeout=2)
        except subprocess.TimeoutExpired:
            os.killpg(process.pid, signal.SIGKILL)
            process.wait(timeout=2)
        except ProcessLookupError:
            pass

incoming = {}
received = queue.Queue()

def read_input(length):
    ready, _, _ = select.select([0], [], [], 15)
    data = os.read(0, length) if ready else b''
    if not data:
        raise RuntimeError('SSH 连接已关闭或传输停滞，运行环境准备已取消。')
    return data

def watch_connection():
    seen = set()
    try:
        while True:
            line = bytearray()
            while not line.endswith(b'\n'):
                line.extend(read_input(1))
                if len(line) > 4096:
                    raise RuntimeError('组件传输头无效。')
            if line == b'.\n':
                continue
            header = json.loads(line)
            index = header['index']
            if type(index) is not int or index not in incoming or index in seen:
                raise RuntimeError('组件传输顺序无效。')
            path, record = incoming[index]
            if header['size'] != record['size']:
                raise RuntimeError('组件传输长度无效。')
            digest = hashlib.sha256()
            remaining = record['size']
            with path.open('xb') as output:
                while remaining:
                    data = read_input(min(65536, remaining))
                    output.write(data)
                    digest.update(data)
                    remaining -= len(data)
            if digest.hexdigest() != record['sha256']:
                raise RuntimeError('传入组件 SHA-256 校验失败，已保留原运行环境。')
            seen.add(index)
            received.put((index, path))
    except Exception as error:
        stopping.set()
        stop_process()
        received.put(error)

def receive_packages(native, packages, archives):
    os_info = {}
    for line in Path('/etc/os-release').read_text().splitlines():
        if '=' in line:
            key, value = line.split('=', 1)
            os_info[key] = value.strip('"')
    if (os_info.get('ID'), os_info.get('VERSION_ID'), native) != ('ubuntu', '20.04', 'arm64'):
        raise RuntimeError('Beta 自动下载目前支持 Ubuntu 20.04 ARM64 控制器；其他环境请先手动准备组件。')
    records = []
    for name, version in sorted(packages.items()):
        record = fields(run(['apt-cache', '-o', 'Dir::Cache::pkgcache=', '-o', 'Dir::Cache::srcpkgcache=',
                             'show', '--no-all-versions', name])[1])
        size = int(record.get('Size', '0'))
        filename, digest = record.get('Filename', ''), record.get('SHA256', '')
        if (record.get('Version') != version or record.get('Architecture') not in (native, 'all') or
                not filename.startswith('pool/') or not re.fullmatch(r'[0-9a-f]{64}', digest) or
                not 0 < size <= 256 * 1024 * 1024):
            raise RuntimeError('APT 缓存缺少完整组件信息：%s；请更新设备软件源索引后重试。' % name)
        records.append({'name': name, 'version': version, 'architecture': record['Architecture'],
                        'filename': filename, 'size': size, 'sha256': digest})
    if sum(record['size'] for record in records) > 512 * 1024 * 1024:
        raise RuntimeError('所需组件超过 512 MiB，请手动准备。')
    incoming.update({index: (archives / ('%03d.deb' % index), record) for index, record in enumerate(records)})
    print('PORTCANVAS_ENV_PLAN=' + json.dumps({'os': os_info['ID'], 'release': os_info['VERSION_ID'],
          'architecture': native, 'packages': records}), flush=True)
    files = []
    while len(files) < len(records):
        try:
            item = received.get(timeout=0.2)
        except queue.Empty:
            if stopping.is_set():
                raise RuntimeError('运行环境准备已取消。')
            continue
        if isinstance(item, Exception):
            raise item
        files.append(item[1])
    return files

def run(arguments, cwd=None, env=None, check=True, timeout=60):
    global active_process
    with process_lock:
        if stopping.is_set():
            raise RuntimeError('运行环境准备已取消。')
        process = subprocess.Popen(arguments, cwd=str(cwd) if cwd else None, env=env,
                                   stdin=subprocess.DEVNULL, stdout=subprocess.PIPE,
                                   stderr=subprocess.PIPE, start_new_session=True)
        active_process = process
    try:
        output, _ = process.communicate(timeout=timeout)
        if stopping.is_set():
            raise RuntimeError('运行环境准备已取消。')
        if check and process.returncode:
            raise RuntimeError('组件准备失败：%s；请检查设备基础工具和软件源索引。' % Path(arguments[0]).name)
        return process.returncode, output.decode('utf-8', 'replace')
    except subprocess.TimeoutExpired:
        stop_process()
        raise RuntimeError('运行环境准备超时，请检查设备基础工具和软件源索引。')
    finally:
        with process_lock:
            active_process = None

def private_env(root):
    env = os.environ.copy()
    if root:
        env['PATH'] = str(root / 'bin') + ':' + env.get('PATH', '/usr/bin:/bin')
    return env

def works(command, env):
    try:
        return run(command, env=env, check=False, timeout=8)[0] == 0
    except (OSError, RuntimeError):
        return False

def sftp_binary(root):
    candidates = ([root / 'bin/sftp-server'] if root else []) + [
        Path('/usr/lib/openssh/sftp-server'), Path('/usr/lib/ssh/sftp-server')]
    return next((str(path) for path in candidates if path.is_file() and os.access(str(path), os.X_OK)), None)

def ready(feature, root):
    env = private_env(root)
    if feature == 'desktop':
        binary = shutil.which('x11vnc', path=env['PATH'])
        if binary and works([binary, '-version'], env):
            return True
        legacy = Path.home() / 'robot-research/wheel-combat-studio/v0.1/runtime'
        binary = legacy / 'usr/bin/x11vnc'
        if binary.is_file():
            libraries = legacy / 'usr/lib' / ('aarch64-linux-gnu' if platform.machine() == 'aarch64' else 'x86_64-linux-gnu')
            env['LD_LIBRARY_PATH'] = str(libraries) + ':' + env.get('LD_LIBRARY_PATH', '')
            return works([str(binary), '-version'], env)
        return False
    if feature == 'terminal':
        return all(shutil.which(name, path=env['PATH']) for name in ('bash', 'base64', 'tmux')) and works(['tmux', '-V'], env)
    if feature == 'files':
        binary = sftp_binary(root)
        return bool(binary and works([binary, '-Q', 'requests'], env))
    binary = shutil.which('ffmpeg', path=env['PATH'])
    return bool(binary and works([binary, '-version'], env)) or works([sys.executable, '-B', '-c', 'import cv2'], env)

def cache_root(base):
    try:
        record = json.loads((base / 'current.json').read_text())
        if not re.fullmatch(r'[a-z0-9]+-[0-9a-f]{32}', record['id']):
            return None
        root = (base / 'versions' / record['id']).resolve()
        root.relative_to((base / 'versions').resolve())
        marker = json.loads((root / 'runtime.json').read_text())
        if marker['schema'] == SCHEMA and marker['machine'] == platform.machine():
            return root
    except (OSError, ValueError, KeyError, TypeError):
        pass
    return None

@contextlib.contextmanager
def runtime_lock(base):
    import fcntl
    with (base / 'prepare.lock').open('a') as lock:
        deadline = time.monotonic() + 30
        while True:
            if stopping.is_set():
                raise RuntimeError('运行环境准备已取消。')
            try:
                fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
                break
            except BlockingIOError:
                if time.monotonic() > deadline:
                    raise RuntimeError('另一客户端正在准备运行环境，请稍后重试。')
                time.sleep(0.2)
        yield

def fields(text):
    result = {}
    for line in text.split('\n\n', 1)[0].splitlines():
        if line[:1].isspace() and result:
            result[key] += ' ' + line.strip()
        elif ':' in line:
            key, value = line.split(':', 1)
            result[key] = value.strip()
    return result

def required_packages(roots):
    native = run(['dpkg', '--print-architecture'])[1].strip()
    installed = set()
    for line in run(['dpkg-query', '-W', '-f=${Package}\t${Architecture}\t${db:Status-Status}\n'])[1].splitlines():
        columns = line.split('\t')
        if len(columns) == 3 and columns[1] in (native, 'all') and columns[2] == 'installed':
            installed.add(columns[0])
    metadata = {}
    def info(name):
        if name not in metadata:
            metadata[name] = fields(run(['apt-cache', '-o', 'Dir::Cache::pkgcache=', '-o', 'Dir::Cache::srcpkgcache=',
                                          'show', '--no-all-versions', name], check=False)[1])
        return metadata[name]
    queue = list(roots)
    selected = {}
    while queue:
        name = queue.pop()
        if name in selected:
            continue
        record = info(name)
        if not record.get('Version'):
            raise RuntimeError('软件源缺少所需组件：%s；请检查设备基础工具和软件源索引。' % name)
        selected[name] = record['Version']
        if len(selected) > 128:
            raise RuntimeError('需要的组件过多，已停止自动准备。')
        for group in (record.get('Pre-Depends', '') + ',' + record.get('Depends', '')).split(','):
            alternatives = [match.group(1) for part in group.split('|')
                            for match in [re.match(r'\s*([a-z0-9][a-z0-9+.-]*)(?::[a-z0-9-]+)?', part)] if match]
            if not alternatives or any(name in installed for name in alternatives):
                continue
            candidate = next((name for name in alternatives if info(name).get('Version')), None)
            if candidate:
                queue.append(candidate)
            else:
                raise RuntimeError('软件源缺少组件依赖，请检查树莓派的软件源。')
    return native, selected

def write_wrappers(tree, destination):
    bin_dir = tree / 'bin'
    if bin_dir.is_symlink():
        raise RuntimeError('软件组件目录异常，已保留原文件。')
    bin_dir.mkdir(exist_ok=True)
    libraries = [str(destination / 'usr/lib' / path.name) for path in (tree / 'usr/lib').glob('*-linux-gnu*')]
    libraries += [str(destination / 'lib' / path.name) for path in (tree / 'lib').glob('*-linux-gnu*')]
    libraries += [str(destination / 'usr/lib'), str(destination / 'lib')]
    binaries = {name: 'usr/bin/' + name for name in ('x11vnc', 'tmux', 'ffmpeg', 'bash', 'base64')}
    binaries['sftp-server'] = 'usr/lib/openssh/sftp-server'
    for name, relative in binaries.items():
        if not (tree / relative).is_file():
            continue
        script = '#!/bin/sh\n'
        script += 'if [ -z "${PORTCANVAS_RUNTIME_LD_SET+x}" ]; then\n'
        script += '  export PORTCANVAS_RUNTIME_LD_SET="${LD_LIBRARY_PATH+x}"\n'
        script += '  export PORTCANVAS_RUNTIME_LD_VALUE="${LD_LIBRARY_PATH-}"\nfi\n'
        script += 'export LD_LIBRARY_PATH=' + shlex.quote(':'.join(libraries)) + '${LD_LIBRARY_PATH:+:"$LD_LIBRARY_PATH"}\n'
        script += 'exec ' + shlex.quote(str(destination / relative)) + ' "$@"\n'
        file = bin_dir / name
        if file.is_symlink():
            raise RuntimeError('软件组件入口异常，已保留原文件。')
        file.write_text(script)
        file.chmod(0o755)

def response(feature, root, prepared=False):
    env = private_env(root)
    return {'ready': True, 'feature': feature, 'prepared': prepared,
            'binDir': str(root / 'bin') if root else None,
            'tmux': shutil.which('tmux', path=env['PATH']), 'sftp': sftp_binary(root)}

def prepare(feature):
    if feature not in PACKAGES:
        raise ValueError('不支持的运行环境请求。')
    base = Path.home() / '.cache/portcanvas/runtime'
    base.resolve().relative_to(Path.home().resolve())
    previous = cache_root(base)
    if ready(feature, previous):
        return response(feature, previous)
    if not all(shutil.which(name) for name in ('apt-cache', 'dpkg-deb', 'dpkg-query', 'dpkg')):
        raise RuntimeError('当前设备缺少组件且不支持自动准备，请使用受支持的 Ubuntu/Debian 环境。')
    base.mkdir(parents=True, exist_ok=True)
    with runtime_lock(base):
        previous = cache_root(base)
        if ready(feature, previous):
            return response(feature, previous)
        stage = Path(tempfile.mkdtemp(prefix='.prepare-', dir=str(base)))
        destination = None
        published = False
        try:
            tree = stage / 'tree'
            if previous:
                shutil.copytree(str(previous), str(tree), symlinks=True)
            else:
                tree.mkdir()
            roots = PACKAGES[feature]
            if feature == 'terminal':
                env = private_env(previous)
                roots = [package for command, package in [('tmux', 'tmux'), ('bash', 'bash'), ('base64', 'coreutils')]
                         if not shutil.which(command, path=env['PATH']) or command == 'tmux' and not works(['tmux', '-V'], env)]
            native, packages = required_packages(roots)
            archives = stage / 'packages'
            archives.mkdir()
            env = dict(os.environ, TMPDIR=str(stage))
            files = receive_packages(native, packages, archives)
            if len(files) != len(packages):
                raise RuntimeError('下载的组件不完整，已保留原运行环境。')
            for package in files:
                record = fields(run(['dpkg-deb', '-f', str(package), 'Package', 'Version', 'Architecture'], env=env)[1])
                if packages.get(record.get('Package')) != record.get('Version') or record.get('Architecture') not in (native, 'all'):
                    raise RuntimeError('下载的组件身份或架构不符，已停止准备。')
                run(['dpkg-deb', '-x', str(package), str(tree)], env=env)
            write_wrappers(tree, tree)
            if not ready(feature, tree):
                raise RuntimeError('组件仍无法启动，请检查设备基础库和系统版本。')
            identifier = native + '-' + uuid.uuid4().hex
            destination = base / 'versions' / identifier
            destination.parent.mkdir(exist_ok=True)
            write_wrappers(tree, destination)
            (tree / 'runtime.json').write_text(json.dumps({'schema': SCHEMA, 'machine': platform.machine(), 'packages': packages}))
            if stopping.is_set():
                raise RuntimeError('运行环境准备已取消。')
            tree.rename(destination)
            temporary = base / ('.current-' + uuid.uuid4().hex + '.json')
            temporary.write_text(json.dumps({'id': identifier}))
            os.replace(str(temporary), str(base / 'current.json'))
            published = True
            return response(feature, destination, True)
        finally:
            if not published and destination is not None and destination.exists():
                destination.resolve().relative_to((base / 'versions').resolve())
                shutil.rmtree(str(destination))
            shutil.rmtree(str(stage))

def main():
    threading.Thread(target=watch_connection, daemon=True).start()
    def interrupted(*_):
        stopping.set()
        stop_process()
        raise SystemExit(1)
    signal.signal(signal.SIGTERM, interrupted)
    signal.signal(signal.SIGHUP, interrupted)
    try:
        result = prepare(json.loads(sys.argv[1])['feature'])
    except Exception as error:
        result = {'ready': False, 'message': str(error)}
    print('PORTCANVAS_ENV=' + json.dumps(result, ensure_ascii=True), flush=True)
    return 0 if result['ready'] else 1

if __name__ == '__main__':
    sys.exit(main())
