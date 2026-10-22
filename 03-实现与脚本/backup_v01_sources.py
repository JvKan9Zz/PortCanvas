from pathlib import Path
import shutil

root=Path(__file__).resolve().parents[1]
assert root==Path('D:/轮式格斗/Project-016-WheelCombatStudio')
source=root/'03-实现与脚本/studio'
assert (source/'src/main.cjs').is_file()
target=root/'05-测试与验证/local/v0.1-source-baseline'
assert not target.exists()
shutil.copytree(source,target,ignore=shutil.ignore_patterns('node_modules'))
assert (target/'src/session.cjs').read_bytes()==(source/'src/session.cjs').read_bytes()
print('V0.1 source baseline saved inside project.')
