"""A single SSH-owned camera stream; no files, listeners or persistent service."""
import json
import os
import re
import select
import struct
import sys
import threading
import time

config=json.loads(sys.argv[1])
path=config['path']
if not re.fullmatch(r'/dev/video[0-9]{1,3}',path):sys.exit('Invalid video device')

def disconnected():
    # The SSH channel keeps stdin open. A closed client must release the camera,
    # even if a driver blocks the thread that is reading frames.
    while True:
        ready,_,_=select.select([0],[],[],15)
        if not ready or not os.read(0,1):os._exit(0)

threading.Thread(target=disconnected,daemon=True).start()
import cv2
capture=cv2.VideoCapture(path,cv2.CAP_V4L2)
try:
    if not capture.isOpened():raise RuntimeError('Cannot open camera; check device access or whether it is in use')
    capture.set(cv2.CAP_PROP_FOURCC,cv2.VideoWriter_fourcc(*'MJPG'))
    capture.set(cv2.CAP_PROP_FRAME_WIDTH,640)
    capture.set(cv2.CAP_PROP_FRAME_HEIGHT,480)
    capture.set(cv2.CAP_PROP_FPS,30)
    capture.set(cv2.CAP_PROP_BUFFERSIZE,4)
    next_frame=0
    while True:
        ok=capture.grab()
        if not ok:raise RuntimeError('Camera stopped returning frames')
        now=time.monotonic()
        if now<next_frame:continue
        ok,frame=capture.retrieve()
        if not ok or frame is None:raise RuntimeError('Camera stopped returning frames')
        height,width=frame.shape[:2]
        scale=min(640/width,480/height,1)
        if scale<1:frame=cv2.resize(frame,(round(width*scale),round(height*scale)),interpolation=cv2.INTER_AREA)
        ok,jpeg=cv2.imencode('.jpg',frame,[cv2.IMWRITE_JPEG_QUALITY,70])
        if not ok:raise RuntimeError('JPEG encoding failed')
        payload=jpeg.tobytes()
        if not 4<=len(payload)<=1048576:raise RuntimeError('Camera frame exceeds size limit')
        sys.stdout.buffer.write(struct.pack('!I',len(payload))+payload)
        sys.stdout.buffer.flush()
        next_frame=time.monotonic()+0.125
except Exception as error:
    print(str(error),file=sys.stderr,flush=True)
    sys.exit(1)
finally:capture.release()
