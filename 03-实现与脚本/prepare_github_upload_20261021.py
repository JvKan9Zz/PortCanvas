from pathlib import Path
import shutil
import subprocess


base = Path('D:/轮式格斗').resolve()
studio = base / 'Project-016-WheelCombatStudio'
vision = base / 'Project-017-Visual-to-3DModel'
match = base / 'Project-018-WheelCombatMatchCode'
index = Path('D:/Codex-WorkSpace/全局工作台.md')
specs = [(studio, 'wheel-combat-studio'), (vision, 'visual-to-3DModel'), (match, 'wheel-combat-match-code')]


def write_new(path, text):
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open('x', encoding='utf-8', newline='\n') as stream:
        stream.write(text.strip() + '\n')


assert all(root.is_relative_to(base) for root, name in specs)
assert not match.exists(), match
write_new(match / 'README.md', '''
# Wheel Combat Match Code

保存用户自己编写的 LUBY 与树莓派比赛代码。组内“树莓派”指创意之星 AI 控制器。

- 项目编号：Project-018；创建日期：2026-10-21。
- GitHub：[JvKan9Zz/wheel-combat-match-code](https://github.com/JvKan9Zz/wheel-combat-match-code)，已创建并通过页面确认是私有仓库。
- 当前状态：只完成仓库和目录说明；尚未导入既有比赛代码、构建、烧录或进行实车测试。

## 代码目录

- [luby/](luby/README.md)：用户编写的 LUBY 固件、工程源码和对应构建说明。
- [raspberry-pi/](raspberry-pi/README.md)：用户编写的树莓派程序，按开发调试、正式比赛和独立测试用途分目录。
- [项目简报](00-项目管理/项目简报.md)：范围与接入要求。
- 07-日志与复盘：建仓、版本、部署和验收记录。

上位机位于独立的 [wheel-combat-studio](https://github.com/JvKan9Zz/wheel-combat-studio) 仓库；视觉场景建模位于 [visual-to-3DModel](https://github.com/JvKan9Zz/visual-to-3DModel)。本仓库承载未来比赛业务代码与两端集成。

后续根据用户指定的实际源码建立同步清单，核实 SDK 引用与部署路径。原有用户代码保持原位，本次建仓不迁移或覆盖已有程序。构建产物按版本另行交付，凭据、设备连接配置和原始视频不提交。
''')
write_new(match / '00-项目管理/项目简报.md', '''
# 项目简报

2026-10-21；Project-018；用户追加要求建立仓库，用于存放自己给 LUBY 和树莓派写的比赛代码。沿用前两个仓库的私有设置。

输入：用户后续明确指定的本人源码与构建配置。输出：两端代码的版本记录、构建与部署说明、真实测试记录。

本次范围：创建私有 GitHub 仓库、本地 Git 项目、两端代码目录说明和日志。当前没有上传或迁移实际比赛程序，不调用板端或电机，不改 SDK、依赖及历史工程。

正式代码导入前核实文件来源、目标目录和导入边界，保持开发、独立传感器测试、比赛程序的用途区别。板端实际根路径为 `/home/bcsh/uptech_vision_ai_class_code/p_wheel_combat/jvk_wheel_conbat`，保留现有拼写；目录内 `debug_program/`、`match_program/`、`test_program/` 分别用于开发、比赛和独立测试。

本仓库是否发布正式版本，以对应源码、实际构建、部署和验收证据为准；文档建档不能作为比赛代码运行通过的依据。
''')
write_new(match / 'luby/README.md', '''
# LUBY 代码

用于用户自己编写的 LUBY 固件与工程。当前尚未导入源码。

后续每个工程保留可重现的构建配置、芯片/工具链要求和接口说明。正式部署前区分静态检查、编译、烧录、通信和实车验收；下载或覆盖设备程序按具体任务授权执行。
''')
write_new(match / 'raspberry-pi/README.md', '''
# 树莓派代码

用于用户自己编写的创意之星 AI 控制器程序。当前尚未导入源码。

按用途在需要时创建 `debug_program/`、`match_program/` 和 `test_program/`，与板端现有约定一致。开发程序不自动覆盖正式比赛程序。

SDK、相机、模型与工作目录以具体程序运行环境核实；保留两份不同的 uptech.py，不通过复制覆盖 SDK 解决导入问题。板端执行按工作区既有双机协作流程。
''')
write_new(match / '.gitignore', (studio / '.gitignore').read_text(encoding='utf-8') + '\n*.axf\n*.o\n*.obj\n*.hex\n*.bin\n*.uvguix.*\n')
write_new(match / 'AGENTS.md', (studio / 'AGENTS.md').read_text(encoding='utf-8').replace('需求、路线和接口草案建档', '比赛代码目录和范围建档'))

for root, name in specs:
    url = 'https://github.com/JvKan9Zz/' + name
    readme = root / 'README.md'
    text = readme.read_text(encoding='utf-8')
    if root != match:
        text = text.replace('GitHub 计划仓库名：`' + name + '`；可见性：私有；远端尚未创建。',
                            'GitHub：[' + 'JvKan9Zz/' + name + '](' + url + ')；已创建并通过页面确认是私有仓库。')
        readme.write_text(text, encoding='utf-8', newline='\n')
    write_new(root / '07-日志与复盘/GitHub建仓记录-2026-10-21.md', f'''
# GitHub 建仓记录 — 2026-10-21

- 真实 GitHub 登录账号通过连接器核实为 JvKan9Zz（与本地 Git 作者名称不是同一字段）。
- 已按用户要求在浏览器创建 `{name}`，仓库页面明确显示 Private；地址：{url}。
- 用户已授权浏览器完成建仓及文档上传；比赛代码仓库由用户在进行中追加。
- 当前连接器可读取登录账号，但可访问仓库列表为空；已创建私有仓库的元数据请求返回 404。不能据此把浏览器中已确认的仓库写成未创建，也不能宣称连接器已经获得访问权限。
- 内置浏览器创建标签和绑定标签连续两次超时，随后 Windows-MCP 读取页面并执行；创建请求后先核实状态，没有重复创建同名仓库。
- 页面切换时曾因前台焦点变化导致等待上传页超时，重新选择 Chrome 并读取新状态后继续。后续输入均使用当前页面状态。
- 本条记录确认建仓结果；文档上传、远端内容和版本同步在执行后另行核查。当前没有应用程序、算法或实车验收结论。
''')

raw = index.read_bytes()
text = raw.decode('utf-8-sig')
assert '| 018 |' not in text
old_row = next(line for line in text.splitlines() if line.startswith('| 017 |'))
newline = '\r\n' if b'\r\n' in raw else '\n'
new_row = '| 018 | `D:/轮式格斗/Project-018-WheelCombatMatchCode` | 规划中，比赛代码目录建档 | [README](D:/轮式格斗/Project-018-WheelCombatMatchCode/README.md) | GitHub 私有仓库已创建；尚未导入既有比赛代码 | 2026-10-21 |'
text = text.replace(old_row, old_row + newline + new_row, 1)
text = text.replace('已登记最大项目编号：017；下一个候选：018', '已登记最大项目编号：018；下一个候选：019', 1)
text = text.replace('本地项目已创建，GitHub 私有远端待连接账户后创建', '本地项目和 GitHub 私有远端已创建；文档上传另验')
text = text.replace('纯视觉场景建模；GitHub 私有远端待创建，尚无算法运行成果', '纯视觉场景建模；GitHub 私有远端已创建，尚无算法运行成果')
backup = studio / '07-日志与复盘/全局工作台-建档前-2026-10-21-远端.md'
assert not backup.exists()
backup.write_bytes(raw)
assert index.read_bytes() == raw
index.write_bytes((b'\xef\xbb\xbf' if raw.startswith(b'\xef\xbb\xbf') else b'') + text.encode('utf-8'))

for root, name in specs:
    def git(*args):
        return subprocess.run(['git', '-C', str(root), *args], check=True, capture_output=True)
    if root == match:
        git('init', '-b', 'main')
    git('add', '.')
    git('diff', '--cached', '--check')
    git('commit', '-m', 'docs: record private GitHub repositories and project scope')
    remotes = git('remote').stdout.decode().splitlines()
    assert 'origin' not in remotes
    git('remote', 'add', 'origin', 'https://github.com/JvKan9Zz/' + name + '.git')
    stage = root / '05-测试与验证/local/github-upload-20261021'
    assert not stage.exists()
    stage.mkdir(parents=True)
    files = git('ls-files', '-z').stdout.decode('utf-8').split('\0')
    files = [name for name in files if name]
    for relative in files:
        target = stage / relative
        target.parent.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(root / relative, target)
    print(name, 'files:', len(files), 'upload directory:', stage)
