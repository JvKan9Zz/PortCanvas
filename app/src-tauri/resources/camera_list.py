import fcntl
import glob
import json
import os
import struct
from pathlib import Path

devices=[]
for path in sorted(glob.glob('/dev/video*')):
    try:
        fd=os.open(path,os.O_RDONLY|os.O_NONBLOCK)
        try:
            info=bytearray(104)
            fcntl.ioctl(fd,0x80685600,info,True)
        finally:os.close(fd)
        capabilities,device_caps=struct.unpack_from('II',info,84)
        effective=device_caps if capabilities&0x80000000 else capabilities
        if not effective&(0x00000001|0x00001000):continue
        name=bytes(info[16:48]).split(b'\0',1)[0].decode('utf-8','replace')
        devices.append({'path':path,'name':name,'accessible':os.access(path,os.R_OK|os.W_OK)})
    except OSError:continue
print(json.dumps(devices,ensure_ascii=False))
