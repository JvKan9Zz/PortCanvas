"""Convert the already bundled licensed faces for the native renderer.

Run with fonttools[woff]. Runtime builds only consume the generated assets.
Dynamic text can use the renderer's system-font fallback for other glyphs.
"""
from pathlib import Path
from html import escape
from io import BytesIO
import subprocess
from fontTools.ttLib import TTFont
from fontTools import subset
from fontTools.pens.svgPathPen import SVGPathPen

app = Path(__file__).resolve().parents[1]
dest = app / 'native-probe/assets'
dest.mkdir(exist_ok=True)
def font_source(name):
    # The old full faces remain recoverable from the immutable stable tag.
    return subprocess.check_output(['git', '-C', str(app.parent), 'show', f'v0.5.95:app/ui/fonts/{name}'])
characters = ''.join(chr(c) for c in range(32, 127))
for path in (app / 'native-probe/src').iterdir():
    if path.suffix in ('.rs', '.slint'):
        characters += path.read_text(encoding='utf-8-sig')
characters += (app / 'core/resources/en.json').read_text(encoding='utf-8-sig')
for face in ('regular', 'medium', 'semibold'):
    font = TTFont(BytesIO(font_source(f'pingfang-{face}.woff2')))
    options = subset.Options()
    sub = subset.Subsetter(options=options)
    sub.populate(text=characters)
    sub.subset(font)
    for record in font['name'].names:
        if record.nameID in (1, 16):
            record.string = 'PortCanvas UI'.encode(record.getEncoding())
    font.flavor = None
    font.save(dest / f'ui-{face}.ttf')
font = TTFont(BytesIO(font_source('macintosh-skeletal-filled-regular.woff2')))
cmap = font.getBestCmap()
glyphs = font.getGlyphSet()
ascent, descent = font['hhea'].ascent, font['hhea'].descent
parts = []
x = 0
for char in 'PortCanvas':
    name = cmap[ord(char)]
    pen = SVGPathPen(glyphs)
    glyphs[name].draw(pen)
    parts.append(f'<path transform="translate({x},0)" d="{escape(pen.getCommands(), quote=True)}"/>')
    x += font['hmtx'].metrics[name][0]
(dest / 'wordmark.svg').write_text(
    f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {x} {ascent-descent}">'
    '<defs><linearGradient id="brand" x1="0" y1=".64" x2="1" y2=".36"><stop stop-color="#0874d1"/><stop offset=".48" stop-color="#35a9ef"/><stop offset="1" stop-color="#ecfaff"/></linearGradient></defs>'
    f'<g fill="url(#brand)" stroke="#2875ad" stroke-width="57" transform="translate(0,{ascent}) scale(1,-1)">{"".join(parts)}</g></svg>\n', encoding='utf-8')
for name in ('NOTICE.txt', 'LICENSE-Macintosh-Skeletal.txt'):
    (dest / name).write_bytes(font_source(name))
print({p.name: p.stat().st_size for p in dest.iterdir()})
