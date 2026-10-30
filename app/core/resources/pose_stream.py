"""Observe one latest pose file; never write telemetry or run robot code."""
import json
import os
import select
import stat
import sys
import threading
import time

config=json.loads(sys.argv[1]);path=config['path']
def disconnected():
    while True:
        ready,_,_=select.select([0],[],[],15)
        if not ready or not os.read(0,1):os._exit(0)
threading.Thread(target=disconnected,daemon=True).start()
signature=None;cached={'source':'missing'};last_emit=0
while True:
    try:
        info=os.stat(path)
        if not stat.S_ISREG(info.st_mode) or info.st_size>4096:raise ValueError('Invalid pose file')
        current=(info.st_ino,info.st_mtime_ns,info.st_size)
        if current!=signature:
            with open(path,'rb') as stream:raw=stream.read(4097)
            if len(raw)>4096:raise ValueError('Pose file too large')
            record=json.loads(raw)
            if not isinstance(record,dict):raise ValueError('Pose must be an object')
            sample={key:record[key] for key in ('schema','x','y','timestamp','heading','valid','confidence') if key in record}
            cached={'source':'sample','sample':sample}
    except FileNotFoundError:
        current=None;cached={'source':'missing'}
    except (OSError,ValueError,TypeError):
        current=('invalid',);cached={'source':'invalid'}
    changed=current!=signature;signature=current;now=time.monotonic()
    if changed or now-last_emit>=0.5:
        try:print(json.dumps(dict(cached,nowMs=time.time()*1000,pid=os.getpid()),allow_nan=False,separators=(',',':')),flush=True)
        except ValueError:print('{"source":"invalid"}',flush=True)
        last_emit=now
    time.sleep(0.1)

