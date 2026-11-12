"""Build the native per-user installer with explicit UTF-8 NSIS input."""
from pathlib import Path
import argparse, os, shutil, subprocess, tempfile

app = Path(__file__).resolve().parents[1]
p = argparse.ArgumentParser()
p.add_argument('--binary', type=Path, required=True)
p.add_argument('--output', type=Path, required=True)
p.add_argument('--version', default='0.6.0-beta.1')
p.add_argument('--version-quad', default='0.6.0.1')
p.add_argument('--product', default='PortCanvas Beta')
p.add_argument('--registry', default='PortCanvasBeta')
p.add_argument('--makensis', type=Path, help='Native NSIS compiler; otherwise use PATH or the existing local NSIS cache')
args = p.parse_args()
binary = args.binary.resolve(strict=True)
output = args.output.resolve()
output.parent.mkdir(parents=True, exist_ok=True)
def string(value): return str(value).replace('$', '$$').replace('"', '$\\"')
defines = {'BINARY':binary,'OUTPUT':output,'VERSION':args.version,'VERSION_QUAD':args.version_quad,'PRODUCT':args.product,'REGKEY':args.registry}
compiler = args.makensis or shutil.which('makensis') or Path(os.environ['LOCALAPPDATA'])/'tauri/NSIS/makensis.exe'
with tempfile.TemporaryDirectory(prefix='native-installer-', dir=output.parent) as temporary:
    source = Path(temporary)/'build.nsi'
    source.write_text(''.join(f'!define {key} "{string(value)}"\n' for key,value in defines.items())+f'!include /CHARSET=UTF8 "{string(app/"native-probe/installer.nsi")}"\n', encoding='utf-8-sig')
    result = subprocess.run([str(compiler),'/V2','/WX','/INPUTCHARSET','UTF8','/OUTPUTCHARSET','UTF8',str(source)], capture_output=True)
    log = result.stdout+result.stderr
    output.with_suffix('.build.log').write_bytes(log)
    print(log.decode('utf-8',errors='replace').strip())
    if result.returncode: raise SystemExit(result.returncode)
print(f'Native installer created: {output.name} ({output.stat().st_size} bytes)')
