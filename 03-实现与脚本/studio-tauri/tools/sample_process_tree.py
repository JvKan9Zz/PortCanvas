"""Record full application process trees; use the same sampler for both versions."""
import argparse,json,time,statistics
from pathlib import Path
import psutil

parser=argparse.ArgumentParser();parser.add_argument('--pid',type=int,required=True);parser.add_argument('--seconds',type=float,default=10);parser.add_argument('--output',type=Path,required=True);parser.add_argument('--label',required=True);args=parser.parse_args()
root=psutil.Process(args.pid);samples=[];previous={};previous_time=None;deadline=time.monotonic()+args.seconds
while time.monotonic()<deadline:
    now=time.monotonic();processes=[root,*root.children(recursive=True)];rows=[];delta=0
    for process in processes:
        try:
            memory=process.memory_info();cpu=process.cpu_times();total=cpu.user+cpu.system
            if previous_time is not None:delta+=max(0,total-previous.get(process.pid,total))
            previous[process.pid]=total
            rows.append({'pid':process.pid,'name':process.name(),'workingSet':memory.rss,'privateBytes':memory.private,'cpuSeconds':total})
        except (psutil.NoSuchProcess,psutil.AccessDenied):continue
    samples.append({'elapsed':now-(deadline-args.seconds),'cpuPercent':None if previous_time is None else 100*delta/(now-previous_time)/(psutil.cpu_count() or 1),'workingSet':sum(p['workingSet'] for p in rows),'privateBytes':sum(p['privateBytes'] for p in rows),'processes':rows});previous_time=now;time.sleep(.25)
result={'label':args.label,'rootPid':args.pid,'rootExecutable':root.exe(),'durationSeconds':args.seconds,'logicalProcessors':psutil.cpu_count(),'psutilVersion':psutil.__version__,'samples':samples,'summary':{'medianWorkingSet':statistics.median(s['workingSet'] for s in samples),'peakWorkingSet':max(s['workingSet'] for s in samples),'medianPrivateBytes':statistics.median(s['privateBytes'] for s in samples),'medianCpuPercent':statistics.median(s['cpuPercent'] for s in samples if s['cpuPercent'] is not None)}}
args.output.parent.mkdir(parents=True,exist_ok=True);args.output.write_text(json.dumps(result,indent=2),encoding='utf-8');print(json.dumps(result['summary']))
