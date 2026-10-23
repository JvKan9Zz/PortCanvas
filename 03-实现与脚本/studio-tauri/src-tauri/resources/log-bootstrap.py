"""Prepare one dedicated logging session without creating board-side log files."""
import base64
import json
import re
import shlex
import subprocess
import sys
import socket

config=json.loads(sys.argv[1])
name=config['session']
if not re.fullmatch(r'wcs-[a-f0-9]{16}-logs',name):
    sys.exit('Invalid Studio session')
base=['tmux','-L','wcs-terminal','-f','/dev/null']
def run(args,check=True):
    result=subprocess.run(base+args,text=True,capture_output=True)
    if check and result.returncode:
        sys.exit('Terminal preparation failed: '+(result.stderr.strip()[:1500] or args[0]))
    return result

action=config.get('action','ensure')
if action not in ('ensure','reset','remove'):sys.exit('Invalid terminal action')
target='='+name
exists=run(['has-session','-t',target],False).returncode==0
if action in ('reset','remove') and exists:
    run(['kill-session','-t',target]);exists=False
if action=='remove':
    print(json.dumps({'session':name,'removed':True}));sys.exit(0)
if not exists:
    rc=base64.b64encode(config['source'].encode()).decode()
    command='HISTFILE=/dev/null exec bash --noprofile --rcfile <(printf %s '+shlex.quote(rc)+' | base64 -d) -i'
    shell='exec bash -c '+shlex.quote(command)
    run(['new-session','-d','-s',name,'-n','studio-log',shell])
    run(['set-window-option','-t',target+':0','automatic-rename','off'])
run(['set-option','-t',target+':0','status','off'])
pane=run(['display-message','-p','-t',target+':0','#{pane_id}']).stdout.strip()
if not re.fullmatch(r'%\d+',pane):
    sys.exit('Logging pane unavailable')
packet=run(['show-option','-wqv','-t',pane,'@wcs_run'],False).stdout.strip()
if packet:
    packet=base64.b64decode(packet).decode('utf-8')
print(json.dumps({'session':name,'pane':pane,'packet':packet,'host':socket.gethostname(),'window':run(['display-message','-p','-t',target+':0','#{window_name}']).stdout.strip()}))
