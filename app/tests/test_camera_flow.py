"""Camera backpressure tests: synthetic JPEG pipe, no device or FFmpeg process."""
import ast
import io
import os
from pathlib import Path
import subprocess
import threading
import time
import types
import unittest

SOURCE=Path(__file__).resolve().parents[1]/'core/resources/camera_stream.py'
class FlowTests(unittest.TestCase):
    def exercise(self,pull,slow):
        read_fd,write_fd=os.pipe()
        stdout=os.fdopen(read_fd,'rb');writer=os.fdopen(write_fd,'wb',buffering=0)
        stop=threading.Event();sent=[]
        process=types.SimpleNamespace(stdout=stdout,stderr=io.BytesIO())
        namespace={'os':os,'sys':__import__('sys'),'struct':__import__('struct'),'threading':threading,'time':time,
            'subprocess':types.SimpleNamespace(Popen=lambda *a,**k:process,DEVNULL=subprocess.DEVNULL,PIPE=subprocess.PIPE),
            'child_lock':threading.Lock(),'stopping':stop,'pull':pull,'credits':threading.Semaphore(2),'path':'/dev/video0','fps':20}
        def send(payload):
            sent.append(payload[2])
            if slow:time.sleep(0.12)
            if len(sent)==5:stop.set()
        namespace['send']=send
        tree=ast.parse(SOURCE.read_text(encoding='utf-8'))
        nodes=[n for n in tree.body if isinstance(n,ast.FunctionDef) and n.name in ('next_deadline','copied_mjpeg')]
        exec(compile(ast.Module(body=nodes,type_ignores=[]),str(SOURCE),'exec'),namespace)
        def produce():
            try:
                for sequence in range(1,100):
                    if stop.is_set():break
                    payload=bytes([255,216,sequence,255,217])
                    writer.write(b'--frame\r\nContent-Length: 5\r\n\r\n'+payload+b'\r\n')
                    time.sleep(1/30)
            except (BrokenPipeError,OSError):pass
            finally:writer.close()
        producer=threading.Thread(target=produce);producer.start()
        errors=[]
        def run():
            try:namespace['copied_mjpeg']('owned-fake-ffmpeg')
            except Exception as error:
                if not stop.is_set():errors.append(error)
        consumer=threading.Thread(target=run);consumer.start()
        try:
            if pull:
                time.sleep(0.35)
                self.assertEqual(len(sent),2,'only two frames may enter transport without receiver credit')
            else:
                consumer.join(2)
                self.assertEqual(len(sent),5)
                self.assertGreaterEqual(sent[-1]-sent[0],8,'slow sender must skip stale capture frames')
        finally:
            stop.set();producer.join(2);consumer.join(2);stdout.close()
        self.assertFalse(consumer.is_alive())
        self.assertEqual(errors,[])
    def test_slow_sender_keeps_latest_capture(self):self.exercise(False,True)
    def test_receiver_credit_bounds_frames_in_transport(self):self.exercise(True,False)
if __name__=='__main__':unittest.main()
