from pathlib import Path
import json
root=Path(__file__).resolve().parents[1]
app=root/'03-实现与脚本/studio'
for name in ['package.json','package-lock.json']:
    file=app/name
    data=json.loads(file.read_text(encoding='utf-8'))
    assert data['version']=='0.1.0'
    data['version']='0.2.0'
    if name=='package.json':data['description']='轮式格斗设备连接、远程桌面与 SFTP 文件传输'
    else:data['packages']['']['version']='0.2.0'
    file.write_text(json.dumps(data,ensure_ascii=False,indent=2)+'\n',encoding='utf-8')
for name,old,new in [('builder.cjs','demo-v0.1','demo-v0.2'),('src/main.cjs','Demo V0.1','Demo V0.2'),('tools/check-package.cjs','demo-v0.1','demo-v0.2')]:
    file=app/name;text=file.read_text(encoding='utf-8');assert old in text
    file.write_text(text.replace(old,new),encoding='utf-8')
print('Version updated to 0.2.0; V0.1 deliverables retained.')
