from pathlib import Path
import json
import subprocess


base = Path('D:/轮式格斗').resolve()
records = [
    ('Project-016-WheelCombatStudio', 'wheel-combat-studio', 11, '4ba7130'),
    ('Project-017-Visual-to-3DModel', 'visual-to-3DModel', 9, '5ae2322'),
    ('Project-018-WheelCombatMatchCode', 'wheel-combat-match-code', 7, '2fa6168'),
]
for directory, repo, count, commit in records:
    root = base / directory
    assert root.resolve().is_relative_to(base)
    files = subprocess.run(['git', '-C', str(root), 'ls-files', '-z'], check=True, capture_output=True).stdout.decode('utf-8').split('\0')
    files = [p for p in files if p]
    assert len(files) == count
    stage = root / '05-测试与验证/local/github-upload-20261021'
    assert all((stage / p).is_file() for p in files)
    report = {
        'date': '2026-10-21',
        'repository_url': f'https://github.com/JvKan9Zz/{repo}',
        'visibility': 'private',
        'default_branch': 'main',
        'browser_commit_short_sha': commit,
        'uploaded_file_count': count,
        'uploaded_paths': files,
        'verification': 'GitHub browser upload list matched the staged tracked files; committed repository page showed main, commit, expected root directories/files and Private.',
        'transport': 'browser upload through Windows-MCP',
        'connector_private_access_verified': False,
        'cli_authenticated_push_verified': False,
        'git_history': 'Local initialization history retained; browser upload created a separate remote initial commit. No force push or local history replacement was performed.',
        'limitations': 'Initial project documentation only; no application, algorithm, firmware, deployment or hardware acceptance.',
    }
    target = root / '05-测试与验证/local/GitHub私有仓库验收-2026-10-21.json'
    assert not target.exists()
    target.write_text(json.dumps(report, ensure_ascii=False, indent=2) + '\n', encoding='utf-8')
    log = root / '07-日志与复盘/GitHub建仓记录-2026-10-21.md'
    with log.open('a', encoding='utf-8') as stream:
        stream.write(f'\n## 上传后验收\n\n已通过浏览器上传 {count} 个初始跟踪文件，远端 main 提交短号 `{commit}`；提交后页面显示预期目录、README 与 Private。逐条待上传路径已与上传副本核对。浏览器拖放跳过了 .gitignore，已使用文件选择器补传，确认列表包含该文件后提交。\n\n本地 origin 已登记真实仓库地址。此次使用网页上传，保留了本地初始化历史，远端另有网页初始提交；未验证命令行推送认证或自动跟踪同步，也未重写历史。当前连接器仍未验证私有仓库访问。详细记录位于本项目 05-测试与验证/local/GitHub私有仓库验收-2026-10-21.json；这份上传后验收是本地新增记录，不属于上述首次远端提交。\n')
    print(repo, count, commit, target)

index = Path('D:/Codex-WorkSpace/全局工作台.md')
text = index.read_text(encoding='utf-8-sig')
text = text.replace('本地项目和 GitHub 私有远端已创建；文档上传另验', 'GitHub 私有 main 已网页提交 11 个初始文件；未实现软件')
text = text.replace('纯视觉场景建模；GitHub 私有远端已创建，尚无算法运行成果', 'GitHub 私有 main 已网页提交 9 个初始文件；尚无算法运行成果')
text = text.replace('GitHub 私有仓库已创建；尚未导入既有比赛代码', 'GitHub 私有 main 已网页提交 7 个初始文件；尚未导入比赛代码')
index.write_text(text, encoding='utf-8', newline='\n')

global_log = Path('D:/Codex-WorkSpace/全局复利与踩坑日志.md')
text = global_log.read_text(encoding='utf-8-sig')
needle = '### E002 统计先验证字段、数值排序与截断'
assert needle in text
note = '''- 2026-10-21 GitHub 网页建仓补充：连接器能返回账号不代表能读写私有仓库，需分别核查仓库访问和实际创建/上传结果。本次先经明确浏览器授权创建三个私有仓库；GUI 拖放保留目录但略过 .gitignore，改用文件选择器单独补传，并核对完整路径清单、提交后的 main/目录/Private。前台焦点会随用户操作或宿主变化，拖放前重新确认 Chrome、资源管理器与源目录。边界：网页提交不等于本地 Git 历史已推送或命令行认证已配置；未获得 GitHub App 仓库访问时不循环重试 404。来源：D:/轮式格斗/Project-016-WheelCombatStudio/07-日志与复盘/GitHub建仓记录-2026-10-21.md，Project-017/018 同类验收。\n\n'''
assert '2026-10-21 GitHub 网页建仓补充' not in text
global_log.write_text(text.replace(needle, note + needle, 1), encoding='utf-8', newline='\n')
