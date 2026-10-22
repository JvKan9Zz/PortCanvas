"""Prepare one dedicated logging session without creating board-side log files."""
import base64
import json
import re
import shlex
import subprocess
import sys

config=json.loads(sys.argv[1])
name=config['session']
if not re.fullmatch(r'wcs-[a-f0-9]{16}-logs',name):
    sys.exit('Invalid Studio session')
base=['tmux','-L','wcs-terminal','-f','/dev/null']
def run(args,check=True):
    return subprocess.run(base+args,text=True,capture_output=True,check=check)

if run(['has-session','-t',name],False).returncode:
    rc=base64.b64encode(config['source'].encode()).decode()
    command='HISTFILE=/dev/null exec bash --noprofile --rcfile <(printf %s '+shlex.quote(rc)+' | base64 -d) -i'
    shell='exec bash -c '+shlex.quote(command)
    run(['new-session','-d','-s',name,'-n','studio-log',shell])
    run(['set-window-option','-t',name+':0','automatic-rename','off'])
pane=run(['display-message','-p','-t',name+':0','#{pane_id}']).stdout.strip()
if not re.fullmatch(r'%\d+',pane):
    sys.exit('Logging pane unavailable')
packet=run(['show-option','-wqv','-t',pane,'@wcs_run'],False).stdout.strip()
if packet:
    packet=base64.b64decode(packet).decode('utf-8')
print(json.dumps({'session':name,'pane':pane,'packet':packet}))
