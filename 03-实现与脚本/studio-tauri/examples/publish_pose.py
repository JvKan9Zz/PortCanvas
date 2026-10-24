"""Call from a real localization program; importing this file publishes nothing."""
import json
import math
import os
import time
from pathlib import Path

def publish_pose(x=None,y=None,heading=None,confidence=None,valid=True,path='/dev/shm/portcanvas-pose.json'):
    record={'schema':'portcanvas.pose/1','valid':bool(valid),'timestamp':time.time()*1000}
    if valid:
        if any(isinstance(n,bool) or not isinstance(n,(int,float)) or not math.isfinite(n) or not 0<=n<=3.8 for n in (x,y)):
            raise ValueError('x/y must be finite arena coordinates in meters')
        record.update(x=x,y=y)
        if heading is not None:record['heading']=heading
        if confidence is not None:record['confidence']=confidence
    payload=json.dumps(record,allow_nan=False,separators=(',',':')).encode('utf-8')
    target=Path(path);temporary=target.with_name(target.name+'.tmp')
    # One latest RAM file, one bounded staging name; one producer per target.
    descriptor=os.open(str(temporary),os.O_WRONLY|os.O_CREAT|os.O_TRUNC|os.O_NOFOLLOW,0o644)
    try:
        with os.fdopen(descriptor,'wb') as stream:stream.write(payload)
        os.replace(str(temporary),str(target))
    finally:
        try:temporary.unlink()
        except FileNotFoundError:pass

