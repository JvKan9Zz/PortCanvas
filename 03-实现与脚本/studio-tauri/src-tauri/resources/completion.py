"""List command/path candidates without evaluating the user's command line."""
import json
import os
import re
import subprocess
import sys

config=json.loads(sys.argv[1])
session=config['session']
if not re.fullmatch(r'wcs-[a-f0-9]{16}-logs',session):sys.exit('Invalid terminal session')
prefix=config['prefix']
if len(prefix.encode('utf-8'))>2048 or any(ord(c)<32 for c in prefix):sys.exit('Invalid completion prefix')
cwd=subprocess.run(['tmux','-L','wcs-terminal','display-message','-p','-t','='+session+':0','#{pane_current_path}'],text=True,capture_output=True,check=True,timeout=3).stdout.strip()
if not os.path.isabs(cwd):sys.exit('Terminal working directory unavailable')
items={}
if config.get('commands') and '/' not in prefix and not prefix.startswith('~'):
    env=dict(os.environ,BASH_ENV='/dev/null',ENV='/dev/null')
    found=subprocess.run(['bash','--noprofile','--norc','-c','compgen -c -- "$1"','portcanvas-completion',prefix],cwd=cwd,env=env,text=True,capture_output=True,timeout=3)
    for name in found.stdout.splitlines():
        if name.startswith(prefix) and not any(ord(c)<32 for c in name):items[name]={'value':name,'label':name,'kind':'command'}
if prefix=='~':
    items['~/']={'value':'~/','label':'~/','kind':'directory'}
else:
    head,leaf=os.path.split(prefix)
    directory=os.path.expanduser(head) if head else cwd
    if not os.path.isabs(directory):directory=os.path.join(cwd,directory)
    try:
        with os.scandir(directory) as entries:
            for entry in entries:
                if not entry.name.startswith(leaf) or any(ord(c)<32 for c in entry.name):continue
                is_dir=entry.is_dir()
                if config.get('directories') and not is_dir:continue
                value=(head+'/' if head and head!='/' else head)+entry.name+('/' if is_dir else '')
                items.setdefault(value,{'value':value,'label':entry.name+('/' if is_dir else ''),'kind':'directory' if is_dir else 'file'})
    except (OSError,ValueError):pass
values=sorted(items.values(),key=lambda item:item['value'])
print(json.dumps({'items':values[:64],'truncated':len(values)>64,'cwd':cwd},ensure_ascii=False))
