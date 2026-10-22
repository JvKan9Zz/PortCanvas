"""Prepare only a private x11vnc runtime; do not install system packages."""
import json
import sys
from pathlib import Path
from board_task import bridge, ROOT

REMOTE = '/home/bcsh/robot-research/wheel-combat-studio/v0.1'

SOURCE = r'''
import json, os, shutil, subprocess
from pathlib import Path
root=Path('/home/bcsh/robot-research/wheel-combat-studio/v0.1')
assert Path.home()==Path('/home/bcsh')
root.relative_to(Path.home()/'robot-research')
root.mkdir(parents=True,exist_ok=True)
packages=root/'packages'
packages.mkdir(exist_ok=True)
runtime=root/'runtime'
runtime.mkdir(exist_ok=True)
binary=runtime/'usr/bin/x11vnc'
if not binary.exists():
    subprocess.run(['apt-get','download','x11vnc','libvncserver1','libvncclient1'],cwd=str(packages),check=True,timeout=180)
    for package in packages.glob('*.deb'):
        subprocess.run(['dpkg-deb','-x',str(package),str(runtime)],check=True,timeout=30)
env=os.environ.copy()
env['LD_LIBRARY_PATH']=str(runtime/'usr/lib/aarch64-linux-gnu')
check=subprocess.run(['ldd',str(binary)],env=env,capture_output=True,text=True,timeout=15)
assert check.returncode==0 and 'not found' not in check.stdout,check.stdout+check.stderr
version=subprocess.run([str(binary),'-version'],env=env,capture_output=True,text=True,timeout=15)
assert version.returncode==0,version.stderr
print('STUDIO_JSON='+json.dumps({'root':str(root),'binary':str(binary),'version':version.stdout+version.stderr,'ldd':check.stdout,'system_packages_installed':False}))
'''

if __name__ == '__main__':
    output=bridge.transfer([bridge.call_python(SOURCE)],'private-vnc-runtime',timeout=240)
    records=[line.split('STUDIO_JSON=',1)[1] for line in output.splitlines() if line.startswith('STUDIO_JSON=')]
    if len(records)!=1:
        raise RuntimeError('Missing preparation result; inspect transport log')
    record=json.loads(records[0])
    dest=ROOT/'05-测试与验证/local/board-runtime.json'
    dest.parent.mkdir(parents=True,exist_ok=True)
    dest.write_text(json.dumps(record,ensure_ascii=False,indent=2),encoding='utf-8')
    print(json.dumps(record,ensure_ascii=False,indent=2))
