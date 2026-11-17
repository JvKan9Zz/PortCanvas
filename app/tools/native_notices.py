"""Collect locked platform native dependency inventories and bundled license texts."""
import argparse, json, re, subprocess
from pathlib import Path

app = Path(__file__).resolve().parents[1]
p = argparse.ArgumentParser()
p.add_argument('--output', type=Path, required=True)
p.add_argument('--inventory', type=Path, required=True)
p.add_argument('--require-license-texts', action='store_true')
p.add_argument('--target', action='append', help='Repeat to union platform dependencies; defaults to Windows x64')
args = p.parse_args()
manifest = app/'native-probe/Cargo.toml'
targets = args.target or ['x86_64-pc-windows-msvc']
packages, selected = {}, set()
for target in targets:
    result = subprocess.run(['cargo','metadata','--offline','--locked','--format-version','1','--filter-platform',target,'--features','gui','--manifest-path',str(manifest)], capture_output=True)
    if result.returncode:
        print(result.stderr.decode('utf-8',errors='replace')[-3000:])
        raise SystemExit(result.returncode)
    metadata = json.loads(result.stdout)
    platform_packages = {item['id']:item for item in metadata['packages']}
    packages.update(platform_packages)
    nodes = {item['id']:item for item in metadata['resolve']['nodes']}
    pending = [metadata['resolve']['root']]
    platform_selected = set()
    while pending:
        identifier = pending.pop()
        if identifier in platform_selected: continue
        platform_selected.add(identifier)
        for dependency in nodes[identifier]['deps']:
            if any(kind.get('kind') != 'dev' for kind in dependency['dep_kinds']):
                pending.append(dependency['pkg'])
    tree = subprocess.check_output(['cargo','tree','--offline','--locked','--target',target,'--features','gui','--edges','normal,build','--prefix','none','--format','{p}','--manifest-path',str(manifest)],text=True,encoding='utf-8')
    active = {match.groups() for line in tree.splitlines() if (match := re.match(r'([\w-]+) v([^\s]+)',line))}
    selected.update(identifier for identifier in platform_selected if (platform_packages[identifier]['name'],platform_packages[identifier]['version']) in active)

texts, text_ids, rows, missing = [], {}, [], []
supplements = app/'native-probe/licenses/upstream'
sources = json.loads((supplements/'sources.json').read_text(encoding='utf-8'))
def repository(value):
    value = (value or '').lower().replace('https://github.com/','').replace('http://github.com/','').rstrip('/')
    return {'brendanzab/gl-rs':'rust-windowing/gl-rs','warp-tech/russh':'eugeny/russh'}.get(value,value)
def retain(path):
    try:
        value = path.read_text(encoding='utf-8-sig').strip()
    except (UnicodeError, OSError):
        return None
    if not value: return None
    if value not in text_ids:
        text_ids[value] = len(texts)+1
        texts.append(value)
    return text_ids[value]

for package in sorted((packages[i] for i in selected),key=lambda x:(x['name'],x['version'])):
    directory = Path(package['manifest_path']).parent
    paths = set()
    if package.get('license_file'): paths.add(directory/package['license_file'])
    for item in directory.iterdir():
        if item.is_file() and item.name.upper().startswith(('LICENSE','LICENCE','COPYING','COPYRIGHT','NOTICE')):
            paths.add(item)
        elif item.is_dir() and item.name.upper() in ('LICENSES','LICENCES'):
            paths.update(path for path in item.rglob('*') if path.is_file())
    if directory in (app/'native-probe',app/'core'): paths.add(app.parent/'LICENSE')
    vcs = directory/'.cargo_vcs_info.json'
    revision = json.loads(vcs.read_text()).get('git',{}).get('sha1') if vcs.exists() else None
    paths.update(supplements/item['path'] for item in sources if item['revision']==revision and repository(item['repo'])==repository(package.get('repository')))
    references = sorted({number for path in sorted(paths) if (number := retain(path)) is not None})
    row = {'name':package['name'],'version':package['version'],'license':package.get('license'),'repository':package.get('repository'),'revision':revision,'texts':references}
    rows.append(row)
    if not references: missing.append({k:row[k] for k in ('name','version','license','repository','revision')})

if args.require_license_texts and missing:
    raise SystemExit('Missing upstream license texts: ' + ', '.join(item['name'] + ' ' + item['version'] for item in missing))

rust = subprocess.check_output(['rustc','--print','sysroot'],text=True,encoding='utf-8').strip()
runtime = Path(rust)/'share/doc/rust'
rust_licenses = sorted({number for name in ('LICENSE-MIT','LICENSE-APACHE','COPYRIGHT') if (number := retain(runtime/name)) is not None})
rust_licenses += sorted({number for item in sources if item['repo']=='rust-lang/rust' and (number := retain(supplements/item['path'])) is not None})
header = ('PortCanvas native third-party notices\n\n'
          f"Locked {', '.join(targets)} normal/build dependency inventory; development-only fixtures are excluded.\n"
          'Build dependencies are listed for attribution and do not imply runtime services.\n'
          'Slint is used under the retained royalty-free desktop terms in LICENSE-Slint.md.\n'
          'AboutSlint is available in Settings > About > Third-party components.\n'
          'Font notices are retained separately in FONT-NOTICES.txt.\n\n')
lines = [header]
for row in rows:
    lines.append(f"{row['name']} {row['version']} | {row['license'] or 'see bundled terms'} | {row['repository'] or ''} | license texts: {', '.join(map(str,row['texts'])) or 'SPDX declaration only'}\n")
lines.append(f"\nRust standard library | license texts: {', '.join(map(str,rust_licenses))}\n")
for number, value in enumerate(texts,1):
    lines.append(f'\n----- License text {number} -----\n{value}\n')
icon_license=app/'native-probe/src/icons/FLUENT-LICENSE.txt'
lines.append('\nMicrosoft Fluent UI System Icons (Settings, Options)\n'+icon_license.read_text(encoding='utf-8').strip()+'\n')
args.output.parent.mkdir(parents=True,exist_ok=True)
args.output.write_text('\n'.join(line.rstrip() for line in ''.join(lines).splitlines())+'\n',encoding='utf-8')
args.inventory.parent.mkdir(parents=True,exist_ok=True)
args.inventory.write_text(json.dumps({'target':targets[0] if len(targets)==1 else 'macOS universal','targets':targets,'packages':rows,'licenseTexts':len(texts),'missingTexts':missing,'rustLicenseTexts':rust_licenses},ensure_ascii=False,indent=2)+'\n',encoding='utf-8')
print(json.dumps({'packages':len(rows),'licenseTexts':len(texts),'missingTexts':missing,'rustLicenseTexts':rust_licenses},ensure_ascii=False))
