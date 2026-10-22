"""Archive this demo and synchronize only the project index/validated experience."""
from pathlib import Path
import json
import shutil
import zipfile

root=Path(__file__).resolve().parents[1]
assert root==Path('D:/轮式格斗/Project-016-WheelCombatStudio')
delivery=root/'06-交付物'
application=delivery/'demo-v0.1/win-unpacked'
assert (application/'Wheel Combat Studio.exe').is_file()
data=json.loads((application/'data/devices.json').read_text(encoding='utf-8'))
assert set(data)=={'devices','hostKeys'}
for device in data['devices']:
    assert set(device)=={'id','name','host','port','username','display','xauthority'}

archive=delivery/'WheelCombatStudio-Demo-V0.1-Windows-x64.zip'
assert not archive.exists(),'Use a new delivery version rather than overwrite an existing archive'
with zipfile.ZipFile(archive,'w',compression=zipfile.ZIP_DEFLATED,compresslevel=6) as z:
    for file in application.rglob('*'):
        if not file.is_file():continue
        relative=file.relative_to(application)
        if relative.parts[0]=='data' and relative.as_posix()!='data/devices.json':continue
        z.write(file,'WheelCombatStudio-Demo-V0.1/'+relative.as_posix())
    for name in ['Demo-V0.1-使用与部署说明.md','Demo-V0.1-验收记录-2026-10-22.md']:
        z.write(delivery/name,'WheelCombatStudio-Demo-V0.1/'+name)
with zipfile.ZipFile(archive) as z:
    assert z.testzip() is None
    assert 'WheelCombatStudio-Demo-V0.1/Wheel Combat Studio.exe' in z.namelist()
    count=len(z.namelist())

workspace=Path('D:/Codex-WorkSpace')
backup=root/'07-日志与复盘/全局文档更新前-2026-10-22'
backup.mkdir(exist_ok=True)
for name in ['全局工作台.md','全局复利与踩坑日志.md']:
    target=workspace/name
    assert target.is_file()
    if not (backup/name).exists():shutil.copy2(target,backup/name)

index=workspace/'全局工作台.md'
text=index.read_text(encoding='utf-8')
rows=text.splitlines()
matches=[i for i,line in enumerate(rows) if line.startswith('| 016 |')]
assert len(matches)==1
rows[matches[0]]='| 016 | `D:/轮式格斗/Project-016-WheelCombatStudio` | Demo V0.1 已完成；长期路线继续 | [README](D:/轮式格斗/Project-016-WheelCombatStudio/README.md) | Windows EXE 真实 SSH/桌面/基本鼠标键盘/只读/断开重连检查通过；文件与擂台后续 | 2026-10-22 |'
index.write_text('\n'.join(rows)+'\n',encoding='utf-8')

experience=workspace/'全局复利与踩坑日志.md'
text=experience.read_text(encoding='utf-8')
addition='- 2026-10-22 Electron 原型分层补充：Electron 42 的 npm 包安装成功后尚无运行二进制，须按该包提供的 install.js 显式下载，再用实际 EXE 启动验收；打包前核对本地 distribution，避免构建器重复下载。Node 下载不继承 curl 代理配置，本次仅在下载子进程配置已确认监听的 HTTP 代理且保留 TLS 校验后成功。VNC 协议握手、SSH fixture 和打包成功仍不能替代真实鼠标键盘；本次已在交付 EXE 中另行验证，并用断开后专属进程为 0 复核生命周期。边界：当前 Electron 42/Node 24/Windows 与板端 X11 原型，不推广为其他版本或性能结论。来源：D:/轮式格斗/Project-016-WheelCombatStudio/07-日志与复盘/demo-v0.1-开发日志-2026-10-22.md。\n\n'
marker='### E014 API 预检要到实际业务端点'
assert marker in text
if addition not in text:text=text.replace(marker,addition+marker,1)
experience.write_text(text,encoding='utf-8')

summary={'date':'2026-10-22','version':'0.1.0','automated_tests_passed':7,'automated_tests_failed':0,'real_exe_desktop_and_basic_input_passed':True,'view_only_passed':True,'page_retention_and_scaling_passed':True,'desktop_and_ssh_reconnect_passed':True,'desktop_stop_owned_processes':0,'device_disconnect_owned_processes':0,'archive':str(archive),'archive_entries':count,'zip_crc_valid':True,'archive_bytes':archive.stat().st_size,'credentials_persisted_in_device_config':False,'not_tested':['NoMachine performance comparison','long-running weak network','cold boot/power loss','Wayland/other devices','Chinese IME/clipboard','Ctrl+Alt+Del/fullscreen dedicated test']}
(root/'05-测试与验证/local/validation-summary.json').write_text(json.dumps(summary,ensure_ascii=False,indent=2),encoding='utf-8')
print(json.dumps(summary,ensure_ascii=False,indent=2))
