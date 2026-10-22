"""Read only the processes belonging to this demo's VNC runtime."""
import json
import sys
from board_task import bridge,ROOT

SOURCE=r'''
import json
from pathlib import Path
runtime='/home/bcsh/robot-research/wheel-combat-studio/v0.1/runtime/'
rows=[]
for folder in Path('/proc').iterdir():
    if not folder.name.isdigit():continue
    try:
        executable=str((folder/'exe').resolve())
        if not executable.startswith(runtime):continue
        args=(folder/'cmdline').read_bytes().decode(errors='replace').strip('\0').split('\0')
        status=(folder/'status').read_text()
        parent=int(next(line for line in status.splitlines() if line.startswith('PPid:')).split()[1])
        rows.append({'pid':int(folder.name),'parent':parent,'executable':executable,'arguments':args})
    except (OSError,StopIteration,ValueError):pass
print('STUDIO_AUDIT='+json.dumps({'owned_vnc_processes':rows,'count':len(rows)}))
'''
output=bridge.transfer([bridge.call_python(SOURCE)],'owned-vnc-lifecycle',timeout=30)
record=json.loads(next(line.split('=',1)[1] for line in output.splitlines() if line.startswith('STUDIO_AUDIT=')))
label=sys.argv[1] if len(sys.argv)>1 else 'current'
assert label.replace('-','').isalnum()
target=ROOT/'05-测试与验证/local'/('vnc-processes-'+label+'.json')
target.write_text(json.dumps(record,ensure_ascii=False,indent=2),encoding='utf-8')
print(json.dumps(record,ensure_ascii=False,indent=2))
