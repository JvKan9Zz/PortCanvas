"""Rebuild only the licensed native UI faces from the retained stable sources.

Run with fonttools[woff], as for native_assets.py. No system fonts are installed.
"""
from io import BytesIO
from pathlib import Path
import subprocess
from fontTools.ttLib import TTFont
from fontTools import subset

app=Path(__file__).resolve().parents[1]
source=app/'native-probe/src'
characters=''.join(chr(c) for c in range(32,127))
for path in source.iterdir():
    if path.suffix in ('.rs','.slint'):characters+=path.read_text(encoding='utf-8-sig')
for pattern in ('plugins/*/manifest.json','plugins/*/ui.json','plugins/*/worker/src/*.rs'):
    for path in sorted(app.parent.glob(pattern)):
        characters+=path.read_text(encoding='utf-8-sig')
characters+=(app/'core/resources/en.json').read_text(encoding='utf-8-sig')
for face in ('regular','medium','semibold'):
    data=subprocess.check_output(['git','show',f'v0.5.95:app/ui/fonts/pingfang-{face}.woff2'],cwd=app.parent)
    font=TTFont(BytesIO(data),recalcTimestamp=False)
    subsetter=subset.Subsetter(options=subset.Options())
    subsetter.populate(text=characters);subsetter.subset(font)
    for name in font['name'].names:
        if name.nameID in (1,16):name.string='PortCanvas UI'.encode(name.getEncoding())
    font.flavor=None
    target=app/f'native-probe/assets/ui-{face}.ttf'
    font.save(target)
    checked=TTFont(target)
    assert checked.getBestCmap() and checked['OS/2'].usWeightClass in (400,500,600)
    print(f'{target.name}: {target.stat().st_size} bytes, {len(checked.getBestCmap())} glyph mappings')
