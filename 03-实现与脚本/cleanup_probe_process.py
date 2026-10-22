"""Stop only orphan children of this task's file-based lifecycle probe."""
import json
from board_task import bridge,ROOT

SOURCE=r'''
import json,os,signal,time
from pathlib import Path
root='/home/bcsh/robot-research/wheel-combat-studio/v0.1'
targets=[]
for folder in Path('/proc').iterdir():
    if not folder.name.isdigit():continue
    try:
        if str((folder/'exe').resolve())!=root+'/runtime/usr/bin/x11vnc':continue
        parent=int(next(s for s in (folder/'status').read_text().splitlines() if s.startswith('PPid:')).split()[1])
        args=(Path('/proc')/str(parent)/'cmdline').read_bytes().decode().strip('\0').split('\0')
        if len(args)>1 and args[1]==root+'/desktop.py':targets.append((int(folder.name),parent))
    except (OSError,ValueError,StopIteration):pass
for child,parent in targets:
    for pid in (child,parent):
        try:os.kill(pid,signal.SIGTERM)
        except ProcessLookupError:pass
deadline=time.monotonic()+4
while time.monotonic()<deadline and any(Path('/proc',str(child)).exists() for child,_ in targets):time.sleep(.1)
for child,parent in targets:
    folder=Path('/proc',str(child))
    if str((folder/'exe').resolve())==root+'/runtime/usr/bin/x11vnc':
        try:os.kill(child,signal.SIGKILL)
        except ProcessLookupError:pass
print('STUDIO_CLEANUP='+json.dumps({'orphan_probe_pairs':targets,'active_ssh_embedded_sessions_touched':False}))
'''
out=bridge.transfer([bridge.call_python(SOURCE)],'cleanup-orphan-probe',timeout=30)
record=json.loads(next(s.split('=',1)[1] for s in out.splitlines() if s.startswith('STUDIO_CLEANUP=')))
(ROOT/'05-测试与验证/local/probe-cleanup.json').write_text(json.dumps(record,indent=2),encoding='utf-8')
print(json.dumps(record,indent=2))
