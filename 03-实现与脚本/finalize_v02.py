"""Package the verified V0.2 and update only the relevant workspace records."""
from pathlib import Path
import json
import shutil
import zipfile

root=Path(__file__).resolve().parents[1]
assert root==Path('D:/轮式格斗/Project-016-WheelCombatStudio')
evidence=root/'05-测试与验证/local'
real=json.loads((evidence/'v02-real-sftp.json').read_text(encoding='utf-8'))
ui=json.loads((evidence/'v02-ui-validation.json').read_text(encoding='utf-8'))
assert real['passed'] and ui['passed'] and ui['desktopRetained']
delivery=root/'06-交付物'
app=delivery/'demo-v0.2/win-unpacked'
assert (app/'Wheel Combat Studio.exe').is_file()
assert (delivery/'demo-v0.1/win-unpacked/Wheel Combat Studio.exe').is_file()
assert (delivery/'WheelCombatStudio-Demo-V0.1-Windows-x64.zip').is_file()
data=json.loads((app/'data/devices.json').read_text(encoding='utf-8'))
assert set(data)=={'devices','hostKeys'}
for device in data['devices']:
    assert set(device)=={'id','name','host','port','username','display','xauthority'}

archive=delivery/'WheelCombatStudio-Demo-V0.2-Windows-x64.zip'
assert not archive.exists(),'Do not overwrite an existing delivery archive'
with zipfile.ZipFile(archive,'w',compression=zipfile.ZIP_DEFLATED,compresslevel=6) as z:
    for file in app.rglob('*'):
        if not file.is_file():continue
        rel=file.relative_to(app)
        if rel.parts[0]=='data' and rel.as_posix()!='data/devices.json':continue
        z.write(file,'WheelCombatStudio-Demo-V0.2/'+rel.as_posix())
    for name in ['Demo-V0.2-使用与部署说明.md','Demo-V0.2-验收记录-2026-10-22.md']:
        z.write(delivery/name,'WheelCombatStudio-Demo-V0.2/'+name)
with zipfile.ZipFile(archive) as z:
    assert z.testzip() is None
    assert 'WheelCombatStudio-Demo-V0.2/Wheel Combat Studio.exe' in z.namelist()
    count=len(z.namelist())

workspace=Path('D:/Codex-WorkSpace')
backup=root/'07-日志与复盘/全局文档更新前-v0.2-2026-10-22'
backup.mkdir(exist_ok=True)
for name in ['全局工作台.md','全局复利与踩坑日志.md']:
    source=workspace/name
    assert source.is_file()
    if not (backup/name).exists():shutil.copy2(source,backup/name)
index=workspace/'全局工作台.md'
rows=index.read_text(encoding='utf-8').splitlines()
matches=[i for i,line in enumerate(rows) if line.startswith('| 016 |')]
assert len(matches)==1
rows[matches[0]]='| 016 | `D:/轮式格斗/Project-016-WheelCombatStudio` | Demo V0.2 已完成；长期路线继续 | [README](D:/轮式格斗/Project-016-WheelCombatStudio/README.md) | 连接/桌面/SFTP 双向文件与目录传输；16 项测试、实板内容与异常、EXE 界面和桌面并行检查通过 | 2026-10-22 |'
index.write_text('\n'.join(rows)+'\n',encoding='utf-8')
experience=workspace/'全局复利与踩坑日志.md'
text=experience.read_text(encoding='utf-8')
addition='- 2026-10-22 SFTP 断线补充：ssh2 1.17 的文件流在关闭通道后继续请求 close/unlink 可能不回调，使 pipeline 等待不结束。仅对本软件独占 SFTP 实例检查通道状态和所有权，让失效资源关闭及时结束；传输先写临时文件，完整关闭后再发布，重连只清理精确登记的本次临时路径。16 项测试及真实板端断线/重连、双向取消/内容检查通过；不能由取消报错就声称原目标已损坏，也不能用模拟覆盖替代真实 UI 确认。适用于该版本的通道生命周期处理，不修改依赖全局，也不承诺崩溃恢复或断点续传。来源：D:/轮式格斗/Project-016-WheelCombatStudio/07-日志与复盘/demo-v0.2-开发日志-2026-10-22.md。\n\n'
marker='### E046 外部计费先分认证域与字段口径'
assert marker in text
if addition not in text:text=text.replace(marker,addition+marker,1)
experience.write_text(text,encoding='utf-8')

summary={'date':'2026-10-22','version':'0.2.0','tests_passed':16,'tests_failed':0,'real_sftp_scenarios':len(real['results']),'ui_checks':ui['results'],'desktop_retained':True,'zip_crc_valid':True,'zip_entries':count,'zip_bytes':archive.stat().st_size,'archive':str(archive),'prior_version_preserved':True,'password_in_device_config':False,'source_package_matches':13,'ui_detector':'degraded regex only; independent source and screenshot review also performed'}
(evidence/'v02-validation-summary.json').write_text(json.dumps(summary,ensure_ascii=False,indent=2),encoding='utf-8')
print(json.dumps(summary,ensure_ascii=False,indent=2))
