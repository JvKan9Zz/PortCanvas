"""Temporary X11 input fixture. No SDK/hardware, files, daemons or system changes."""
import ctypes as C
import os,sys,time,threading
from pathlib import Path

os.environ['DISPLAY']=os.environ.get('DISPLAY',':0')
if not os.environ.get('XAUTHORITY'):
    for candidate in [Path('/run/user')/str(os.getuid())/'gdm/Xauthority',Path.home()/'.Xauthority']:
        if candidate.is_file():os.environ['XAUTHORITY']=str(candidate);break
x=C.CDLL('libX11.so.6');pointer=C.c_void_p;window=C.c_ulong
def bind(name,arguments,result=C.c_int):
    function=getattr(x,name);function.argtypes=arguments;function.restype=result;return function
bind('XOpenDisplay',[C.c_char_p],pointer);bind('XDefaultRootWindow',[pointer],window)
bind('XCreateSimpleWindow',[pointer,window,C.c_int,C.c_int,C.c_uint,C.c_uint,C.c_uint,C.c_ulong,C.c_ulong],window)
for name in ['XMapWindow','XRaiseWindow','XDestroyWindow']:bind(name,[pointer,window])
bind('XStoreName',[pointer,window,C.c_char_p]);bind('XSelectInput',[pointer,window,C.c_long]);bind('XFlush',[pointer]);bind('XPending',[pointer]);bind('XNextEvent',[pointer,pointer]);bind('XLookupKeysym',[pointer,C.c_int],C.c_ulong);bind('XCloseDisplay',[pointer])
bind('XTranslateCoordinates',[pointer,window,window,C.c_int,C.c_int,C.POINTER(C.c_int),C.POINTER(C.c_int),C.POINTER(window)])
display=x.XOpenDisplay(None)
if not display:sys.exit('X11 input fixture cannot open display')
root=x.XDefaultRootWindow(display);test=x.XCreateSimpleWindow(display,root,100,100,400,220,1,0xeeeeee,0x114477)
stop=threading.Event()
def input_closed():
    try:
        while sys.stdin.buffer.read(1):pass
    finally:stop.set()
threading.Thread(target=input_closed,daemon=True).start()
try:
    x.XStoreName(display,test,b'PortCanvas M2 INPUT TEST');x.XSelectInput(display,test,1|4|32768|131072);x.XMapWindow(display,test);x.XRaiseWindow(display,test);x.XFlush(display)
    event=(C.c_long*24)();deadline=time.monotonic()+180;geometry_at=time.monotonic()+1
    while not stop.is_set() and time.monotonic()<deadline:
        while x.XPending(display):
            x.XNextEvent(display,C.byref(event));kind=C.cast(C.byref(event),C.POINTER(C.c_int)).contents.value
            if kind==2:print('KEY='+str(x.XLookupKeysym(C.byref(event),0)),flush=True)
            if kind==4:print('BUTTON=1',flush=True)
        if geometry_at and time.monotonic()>=geometry_at:
            px,py,child=C.c_int(),C.c_int(),window();x.XTranslateCoordinates(display,test,root,0,0,C.byref(px),C.byref(py),C.byref(child));print('GEOMETRY='+','.join(map(str,[px.value,py.value,400,220])),flush=True);geometry_at=None
        time.sleep(.02)
finally:
    x.XDestroyWindow(display,test);x.XFlush(display);x.XCloseDisplay(display);print('FIXTURE_CLOSED',flush=True)
