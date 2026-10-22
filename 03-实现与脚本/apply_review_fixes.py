from pathlib import Path
import shutil

root=Path(__file__).resolve().parents[1]
file=root/'03-实现与脚本/studio/ui/style.css'
text=file.read_text(encoding='utf-8')
changes={
    '--muted:#65737e':'--muted:#596c78',
    'input::placeholder{color:#77848f}':'input::placeholder{color:#65737e}',
    'color:#74828d':'color:#5e707b',
    'color:#647681':'color:#596d79',
    'color:#677c88':'color:#596d79',
    'color:#6f7f8a':'color:#5f707c',
    'color:#8797a2':'color:#5f707c'
}
for old,new in changes.items():
    assert text.count(old)==1,old
    text=text.replace(old,new)
screens=root/'05-测试与验证/local/screenshots'
backup=screens/'pre-review'
backup.mkdir(exist_ok=True)
for source in screens.glob('*'):
    if source.is_file() and not (backup/source.name).exists():shutil.copy2(source,backup/source.name)
file.write_text(text,encoding='utf-8')
print('Updated auxiliary text contrast; retained pre-review screenshots.')
