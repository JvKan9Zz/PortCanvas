"""SSH-owned preview: copy native MJPEG when supported; never save video."""
import errno
import fcntl
import json
import os
import re
import select
import shutil
import struct
import subprocess
import sys
import threading
import time

config=json.loads(sys.argv[1]);path=config['path'];fps=int(config.get('fps',20))
if not re.fullmatch(r'/dev/video[0-9]{1,3}',path):sys.exit('Invalid video device')
if fps not in (8,15,20,30):sys.exit('Invalid frame rate')
child=None
child_lock=threading.Lock()
stopping=threading.Event()

def stop_child():
    with child_lock:process=child
    if process is not None and process.poll() is None:
        process.terminate()
        try:process.wait(timeout=0.6)
        except subprocess.TimeoutExpired:
            process.kill();process.wait(timeout=0.6)

def disconnected():
    while True:
        ready,_,_=select.select([0],[],[],15)
        if not ready or not os.read(0,1):
            stopping.set();stop_child();os._exit(0)

def native_mjpeg():
    fd=os.open(path,os.O_RDONLY|os.O_NONBLOCK)
    try:
        for index in range(64):
            descriptor=bytearray(64);struct.pack_into('II',descriptor,0,index,1)
            try:fcntl.ioctl(fd,0xc0405602,descriptor,True)
            except OSError as error:
                if error.errno==errno.EINVAL:return False
                raise
            if bytes(descriptor[44:48])==b'MJPG':return True
        return False
    finally:os.close(fd)

def send(payload):
    # This UVC camera pads otherwise valid MJPEG frames with zero bytes.
    payload=payload.rstrip(b'\x00')
    if not 4<=len(payload)<=1048576 or not payload.startswith(b'\xff\xd8') or not payload.endswith(b'\xff\xd9'):
        raise RuntimeError('Invalid camera JPEG')
    sys.stdout.buffer.write(struct.pack('!I',len(payload))+payload);sys.stdout.buffer.flush()

def next_deadline(now,previous):
    return previous+1/fps if now-previous<1/fps else now+1/fps

def copied_mjpeg(executable,copy=True):
    global child
    command=[executable,'-nostdin','-hide_banner','-loglevel','error','-f','v4l2']
    if copy:command+=['-input_format','mjpeg']
    command+=['-framerate','30','-video_size','640x480','-i',path,'-an']
    command+=['-c:v','copy','-bsf:v','mjpeg2jpeg'] if copy else ['-c:v','mjpeg','-q:v','6','-threads','1']
    command+=['-f','mpjpeg','-flush_packets','1','pipe:1']
    with child_lock:
        if stopping.is_set():return
        child=subprocess.Popen(command,stdin=subprocess.DEVNULL,stdout=subprocess.PIPE,stderr=subprocess.PIPE,bufsize=65536)
    diagnostic=bytearray();process=child
    def errors():
        while True:
            data=process.stderr.read(512)
            if not data:break
            diagnostic.extend(data[:max(0,3500-len(diagnostic))])
    threading.Thread(target=errors,daemon=True).start()
    print('PORTCANVAS_CAMERA:'+('ffmpeg-copy' if copy else 'ffmpeg-encode'),file=sys.stderr,flush=True)
    reader=process.stdout;deadline=time.monotonic()
    while True:
        length=None
        for _ in range(12):
            line=reader.readline(1025)
            if not line:raise RuntimeError(diagnostic.decode('utf-8','replace').strip() or 'Camera capture ended')
            if len(line)>1024:raise RuntimeError('Invalid camera multipart header')
            if line.lower().startswith(b'content-length:'):length=int(line.split(b':',1)[1].strip())
            if line in (b'\r\n',b'\n') and length is not None:break
        else:raise RuntimeError('Invalid camera multipart header')
        if length is None or not 4<=length<=1048576:raise RuntimeError('Invalid camera frame size')
        payload=bytearray()
        while len(payload)<length:
            block=reader.read(length-len(payload))
            if not block:raise RuntimeError('Truncated camera frame')
            payload.extend(block)
        now=time.monotonic()
        if now>=deadline:send(payload);deadline=next_deadline(now,deadline)

def opencv_preview():
    import cv2
    capture=cv2.VideoCapture(path,cv2.CAP_V4L2)
    try:
        if not capture.isOpened():raise RuntimeError('Cannot open camera; check device access or whether it is in use')
        capture.set(cv2.CAP_PROP_FOURCC,cv2.VideoWriter_fourcc(*'MJPG'))
        capture.set(cv2.CAP_PROP_FRAME_WIDTH,640);capture.set(cv2.CAP_PROP_FRAME_HEIGHT,480)
        capture.set(cv2.CAP_PROP_FPS,30);capture.set(cv2.CAP_PROP_BUFFERSIZE,4)
        print('PORTCANVAS_CAMERA:opencv',file=sys.stderr,flush=True)
        deadline=time.monotonic()
        while True:
            if not capture.grab():raise RuntimeError('Camera stopped returning frames')
            now=time.monotonic()
            if now<deadline:continue
            ok,frame=capture.retrieve()
            if not ok or frame is None:raise RuntimeError('Camera stopped returning frames')
            height,width=frame.shape[:2];scale=min(640/width,480/height,1)
            if scale<1:frame=cv2.resize(frame,(round(width*scale),round(height*scale)),interpolation=cv2.INTER_AREA)
            ok,jpeg=cv2.imencode('.jpg',frame,[cv2.IMWRITE_JPEG_QUALITY,70])
            if not ok:raise RuntimeError('JPEG encoding failed')
            send(jpeg.tobytes());deadline=next_deadline(now,deadline)
    finally:capture.release()

threading.Thread(target=disconnected,daemon=True).start()
try:
    executable=shutil.which('ffmpeg')
    if executable and config.get('backend')!='opencv' and native_mjpeg():copied_mjpeg(executable)
    else:
        try:import cv2
        except Exception:
            if executable:copied_mjpeg(executable,False)
            else:raise RuntimeError('缺少 FFmpeg/OpenCV，请重新连接并准备远端运行环境')
        else:opencv_preview()
except Exception as error:
    print(str(error),file=sys.stderr,flush=True);sys.exit(1)
finally:stop_child()
