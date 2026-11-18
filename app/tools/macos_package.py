#!/usr/bin/env python3
"""Bundle verified Mach-O slices as an ad-hoc signed, unnotarized test app."""
import argparse
import gzip
import hashlib
import json
import os
import plistlib
import re
import shutil
import subprocess
import tarfile
import tempfile
from pathlib import Path, PurePosixPath

APP = Path(__file__).resolve().parents[1]
BUNDLE_ID = 'com.portcanvas.desktop'
MINIMUM_OS = '13.0'


def bundle_metadata(version):
    match = re.fullmatch(r'(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)(?:-beta\.([1-9][0-9]*))?', version)
    if not match or (match[4] and int(match[4]) > 255):
        raise ValueError('version must be x.y.z or x.y.z-beta.N (Apple beta counter 1–255)')
    base = '.'.join(match.group(i) for i in (1, 2, 3))
    beta = match[4]
    name = 'PortCanvas Beta' if beta else 'PortCanvas'
    return {'CFBundleName': name, 'CFBundleDisplayName': name,
            'CFBundleIdentifier': BUNDLE_ID + ('.beta' if beta else ''),
            'CFBundleShortVersionString': base, 'CFBundleVersion': base + ('b' + beta if beta else ''),
            'PortCanvasVersion': version}


def run(*command):
    return subprocess.check_output([str(value) for value in command], text=True, stderr=subprocess.STDOUT)


def minimum_versions(load_commands):
    pattern = (r'\bcmd LC_BUILD_VERSION\b(?:(?!\bcmd ).)*?\bminos ([\d.]+)|'
               r'\bcmd LC_VERSION_MIN_MACOSX\b(?:(?!\bcmd ).)*?\bversion ([\d.]+)')
    return [modern or legacy for modern, legacy in re.findall(pattern, load_commands, re.S)]


def is_system_dependency(dependency):
    path = PurePosixPath(dependency)
    return '..' not in path.parts and (dependency.startswith('/System/Library/') or dependency.startswith('/usr/lib/'))


def new_output_path(path):
    path = Path(os.path.abspath(path))
    if path.exists() or path.is_symlink():
        raise ValueError(f'Output already exists; choose a new directory: {path}')
    if not path.parent.is_dir():
        raise ValueError(f'Output parent must already exist: {path.parent}')
    return path


def inspect_binary(binary, expected_arches):
    arches = run('/usr/bin/lipo', '-archs', binary).strip().split()
    if set(arches) != set(expected_arches):
        raise ValueError(f'Unexpected architectures in {binary}: {arches}; expected {expected_arches}')
    slices = {}
    for arch in arches:
        commands = run('/usr/bin/otool', '-arch', arch, '-l', binary)
        versions = minimum_versions(commands)
        if len(versions) != 1 or tuple(map(int, versions[0].split('.'))) not in ((13, 0), (13, 0, 0)):
            raise ValueError(f'{arch} must target macOS 13.0, found {versions}')
        dependencies = []
        for line in run('/usr/bin/otool', '-arch', arch, '-L', binary).splitlines():
            if line.startswith(('\t', ' ')) and ' (compatibility version ' in line:
                dependency = line.strip().split(' (compatibility version ', 1)[0]
                if not is_system_dependency(dependency):
                    raise ValueError(f'Non-system runtime dependency: {dependency}')
                dependencies.append(dependency)
        if not dependencies:
            raise ValueError(f'No dynamic system dependencies found in {arch} binary')
        slices[arch] = {'minimumOS': versions[0], 'dependencies': dependencies}
    return slices


def create_archive(app, output, epoch):
    """No AppleDouble files, symlinks, extended attributes or external paths."""
    paths = [app, *sorted(app.rglob('*'))]
    for path in paths:
        if path.is_symlink() or not (path.is_dir() or path.is_file()):
            raise ValueError(f'Archive requires regular files/directories, not links: {path}')
    with output.open('xb') as raw, gzip.GzipFile(filename='', mode='wb', fileobj=raw, mtime=epoch) as zipped:
        with tarfile.open(fileobj=zipped, mode='w', format=tarfile.USTAR_FORMAT) as archive:
            for path in paths:
                info = archive.gettarinfo(str(path), str(path.relative_to(app.parent)))
                info.uid = info.gid = 0
                info.uname = info.gname = ''
                info.mtime = epoch
                info.mode = 0o755 if path.is_dir() or os.access(path, os.X_OK) else 0o644
                with path.open('rb') if path.is_file() else open(os.devnull, 'rb') as content:
                    archive.addfile(info, content if path.is_file() else None)


def copy_resources(resources, notices, inventory):
    files = {
        'LICENSE.txt': APP.parent / 'LICENSE',
        'LICENSE-Slint.md': APP / 'native-probe/LICENSE-Slint.md',
        'FONT-NOTICES.txt': APP / 'native-probe/assets/NOTICE.txt',
        'LICENSE-Macintosh-Skeletal.txt': APP / 'native-probe/assets/LICENSE-Macintosh-Skeletal.txt',
        'FLUENT-LICENSE.txt': APP / 'native-probe/src/icons/FLUENT-LICENSE.txt',
        'THIRD-PARTY-NOTICES.txt': notices,
        'DEPENDENCIES.json': inventory,
    }
    for name, source in files.items():
        if not source.is_file() or source.stat().st_size == 0:
            raise ValueError(f'Missing or empty bundled license/resource: {source}')
        shutil.copyfile(source, resources / name)


def make_icon(resources, scratch):
    iconset = scratch / 'PortCanvas.iconset'
    iconset.mkdir()
    source = APP / 'native-probe/assets/portcanvas.png'
    for size in (16, 32, 128, 256, 512):
        for scale in (1, 2):
            suffix = '@2x' if scale == 2 else ''
            run('/usr/bin/sips', '-z', size * scale, size * scale, source, '--out', iconset / f'icon_{size}x{size}{suffix}.png')
    run('/usr/bin/iconutil', '-c', 'icns', iconset, '-o', resources / 'PortCanvas.icns')


def validate_bundle(app, version):
    with (app / 'Contents/Info.plist').open('rb') as stream:
        info = plistlib.load(stream)
    expected = {**bundle_metadata(version), 'CFBundleExecutable': 'PortCanvasNative',
                'LSMinimumSystemVersion': MINIMUM_OS, 'CFBundleIconFile': 'PortCanvas.icns'}
    for key, value in expected.items():
        if info.get(key) != value:
            raise ValueError(f'Unexpected Info.plist {key}: {info.get(key)}')
    for name in ('PortCanvas.icns', 'LICENSE.txt', 'LICENSE-Slint.md', 'FONT-NOTICES.txt',
                 'LICENSE-Macintosh-Skeletal.txt', 'FLUENT-LICENSE.txt', 'THIRD-PARTY-NOTICES.txt', 'DEPENDENCIES.json', 'BUILD-INFO.json'):
        path = app / 'Contents/Resources' / name
        if not path.is_file() or path.stat().st_size == 0:
            raise ValueError(f'Missing resource: {path}')
    slices = inspect_binary(app / 'Contents/MacOS/PortCanvasNative', ['arm64', 'x86_64'])
    run('/usr/bin/codesign', '--verify', '--deep', '--strict', '--verbose=2', app)
    signature = run('/usr/bin/codesign', '-d', '--verbose=4', app)
    if 'Signature=adhoc' not in signature or f"Identifier={expected['CFBundleIdentifier']}" not in signature.splitlines():
        raise ValueError(f'Expected identified ad-hoc code signature: {signature}')
    return {'architectures': slices, 'signature': 'ad-hoc', 'notarized': False,
            'bundleIdentifier': expected['CFBundleIdentifier'], 'version': version}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--arm64', required=True, type=Path)
    parser.add_argument('--x86-64', required=True, type=Path)
    parser.add_argument('--notices', required=True, type=Path)
    parser.add_argument('--inventory', required=True, type=Path)
    parser.add_argument('--output', required=True, type=Path, help='New directory; existing output is never overwritten')
    parser.add_argument('--version', required=True)
    parser.add_argument('--source-revision', required=True)
    parser.add_argument('--source-state', choices=('clean', 'modified', 'unknown'), default='unknown')
    parser.add_argument('--source-date-epoch', required=True, type=int)
    parser.add_argument('--no-dmg', action='store_true', help='Archive-only CI/packager checks; not the final delivery')
    args = parser.parse_args()
    try:
        metadata = bundle_metadata(args.version)
    except ValueError as error:
        parser.error(str(error))
    output = new_output_path(args.output)
    inspect_binary(args.arm64, ['arm64'])
    inspect_binary(args.x86_64, ['x86_64'])
    with tempfile.TemporaryDirectory(prefix='.PortCanvas-package-', dir=output.parent) as temporary:
        scratch = Path(temporary)
        delivery = scratch / 'delivery'
        delivery.mkdir()
        bundle = delivery / 'PortCanvas.app'
        resources = bundle / 'Contents/Resources'
        executable = bundle / 'Contents/MacOS/PortCanvasNative'
        executable.parent.mkdir(parents=True)
        resources.mkdir()
        run('/usr/bin/lipo', '-create', args.arm64, args.x86_64, '-output', executable)
        executable.chmod(0o755)
        plist = {**metadata, 'CFBundleExecutable': executable.name,
                 'CFBundlePackageType': 'APPL', 'CFBundleIconFile': 'PortCanvas.icns',
                 'LSMinimumSystemVersion': MINIMUM_OS, 'NSHighResolutionCapable': True,
                 'NSPrincipalClass': 'NSApplication',
                 'NSLocalNetworkUsageDescription': 'PortCanvas discovers and connects to your Linux devices on the local network.', 'NSSupportsAutomaticGraphicsSwitching': True,
                 'NSHumanReadableCopyright': 'PortCanvas contributors; third-party terms in Contents/Resources'}
        with (bundle / 'Contents/Info.plist').open('wb') as stream:
            plistlib.dump(plist, stream, sort_keys=True)
        copy_resources(resources, args.notices, args.inventory)
        make_icon(resources, scratch)
        build_info = {'version': args.version, 'sourceRevision': args.source_revision,
                      'sourceState': args.source_state, 'sourceDateEpoch': args.source_date_epoch, 'minimumOS': MINIMUM_OS,
                      'signature': 'ad-hoc', 'notarized': False,
                      'purpose': 'macOS native test build; not an official release',
                      'buildHost': {'macOS': run('/usr/bin/sw_vers', '-productVersion').strip(),
                                    'architecture': run('/usr/bin/uname', '-m').strip(),
                                    'sdk': run('/usr/bin/xcrun', '--sdk', 'macosx', '--show-sdk-version').strip(),
                                    'rustc': run('rustc', '--version').strip()}}
        (resources / 'BUILD-INFO.json').write_text(json.dumps(build_info, indent=2) + '\n')
        run('/usr/bin/codesign', '--force', '--sign', '-', '--timestamp=none', '--identifier', metadata['CFBundleIdentifier'], bundle)
        validation = validate_bundle(bundle, args.version)
        stem = f'PortCanvas_{args.version}_universal'
        archive = delivery / f'{stem}.app.tar.gz'
        create_archive(bundle, archive, args.source_date_epoch)
        # Verify the updater-facing tar has only the agreed single root and regular entries.
        with tarfile.open(archive) as stream:
            for member in stream:
                if not (member.name == 'PortCanvas.app' or member.name.startswith('PortCanvas.app/')) or not (member.isfile() or member.isdir()):
                    raise ValueError(f'Unexpected updater archive member: {member.name}')
        if not args.no_dmg:
            disk_root = scratch / 'disk'
            disk_root.mkdir()
            shutil.copytree(bundle, disk_root / (metadata['CFBundleName'] + '.app'))
            (disk_root / 'Applications').symlink_to('/Applications')
            dmg = delivery / f'{stem}.dmg'
            run('/usr/bin/hdiutil', 'create', '-volname', metadata['CFBundleName'], '-srcfolder', disk_root,
                '-format', 'UDZO', '-ov', dmg)
            run('/usr/bin/hdiutil', 'verify', dmg)
            validation['diskImage'] = 'hdiutil verify passed'
        validation.update(build_info)
        (delivery / 'validation.json').write_text(json.dumps(validation, indent=2) + '\n')
        files = sorted(path for path in delivery.iterdir() if path.is_file())
        (delivery / 'SHA256SUMS').write_text(''.join(f'{hashlib.sha256(path.read_bytes()).hexdigest()}  {path.name}\n' for path in files))
        # Refuse concurrent/accidental replacement even after the expensive build.
        new_output_path(output)
        delivery.rename(output)
    print(f'Ad-hoc signed, not notarized macOS test artifacts: {output}')


if __name__ == '__main__':
    main()
