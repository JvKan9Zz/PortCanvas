import json
import os
import time
from pathlib import Path

cpu=[int(n) for n in Path('/proc/stat').read_text().splitlines()[0].split()[1:9]]
memory={line.split(':')[0]:int(line.split()[1])*1024 for line in Path('/proc/meminfo').read_text().splitlines() if ':' in line}
network=[]
for line in Path('/proc/net/dev').read_text().splitlines()[2:]:
    name,values=line.split(':',1)
    if name.strip()=='lo':continue
    numbers=values.split()
    network.append({'name':name.strip(),'received':int(numbers[0]),'sent':int(numbers[8])})
print(json.dumps({'clock':time.monotonic(),'cpuTotal':sum(cpu),'cpuIdle':cpu[3]+cpu[4],'cores':os.cpu_count(),'memoryTotal':memory['MemTotal'],'memoryAvailable':memory['MemAvailable'],'swapTotal':memory['SwapTotal'],'swapFree':memory['SwapFree'],'uptime':float(Path('/proc/uptime').read_text().split()[0]),'load':os.getloadavg(),'network':network}))
