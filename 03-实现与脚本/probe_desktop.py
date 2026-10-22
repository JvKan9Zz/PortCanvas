"""Finite real-board lifecycle check; no GUI input or hardware access."""
import json
from board_task import bridge,ROOT
from prepare_board import REMOTE

SOURCE=r'''
import json, os, selectors, socket, subprocess, time, threading
from pathlib import Path
root=Path('/home/bcsh/robot-research/wheel-combat-studio/v0.1')
process=subprocess.Popen(['python3',str(root/'desktop.py'),json.dumps({'display':':0','xauthority':''})],stdin=subprocess.PIPE,stdout=subprocess.PIPE,stderr=subprocess.PIPE,text=True)
diagnostic=[]
def drain_stderr():
    for line in process.stderr:
        diagnostic.append(line)
        if len(diagnostic)>120:del diagnostic[:-120]
threading.Thread(target=drain_stderr,daemon=True).start()
selector=selectors.DefaultSelector()
selector.register(process.stdout,selectors.EVENT_READ)
try:
    events=selector.select(timeout=18)
    if not events:
        process.stdin.close();process.wait(timeout=5)
        raise RuntimeError(''.join(diagnostic)[-4000:] or 'Desktop readiness timeout')
    line=process.stdout.readline()
    assert line.startswith('STUDIO_DESKTOP='),line+''.join(diagnostic)[-4000:]
    info=json.loads(line.split('=',1)[1]);port=info['port']
    with socket.create_connection(('127.0.0.1',port),timeout=4) as connection:
        banner=connection.recv(32).decode('ascii')
    assert banner.startswith('RFB '),banner
    process.stdin.close();code=process.wait(timeout=5)
    with socket.socket() as check:
        stopped=check.connect_ex(('127.0.0.1',port))!=0
    assert stopped,'Owned desktop port remained open'
    print('STUDIO_PROBE='+json.dumps({'display':info['display'],'banner':banner,'port':port,'channel_eof_stopped_service':stopped,'returncode':code}))
finally:
    if process.poll() is None:
        process.terminate();process.wait(timeout=5)
'''

output=bridge.transfer(['put -transfer=binary '+bridge.quote(ROOT/'03-实现与脚本/studio/src/desktop.py')+' '+bridge.quote(REMOTE+'/desktop.py'),bridge.call_python(SOURCE)],'desktop-lifecycle',timeout=60)
rows=[line.split('STUDIO_PROBE=',1)[1] for line in output.splitlines() if line.startswith('STUDIO_PROBE=')]
if len(rows)!=1:raise RuntimeError('Missing desktop probe response')
record=json.loads(rows[0]);(ROOT/'05-测试与验证/local/desktop-lifecycle.json').write_text(json.dumps(record,ensure_ascii=False,indent=2),encoding='utf-8')
print(json.dumps(record,ensure_ascii=False,indent=2))
