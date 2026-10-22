const fs = require('node:fs');
const path = require('node:path');
const assert = require('node:assert/strict');
const asar = require('@electron/asar');
const root = path.resolve(__dirname,'..');
const archive = path.resolve(root,'../../06-交付物/demo-v0.2/win-unpacked/resources/app.asar');
const files = ['src/main.cjs','src/preload.cjs','src/session.cjs','src/profiles.cjs','src/desktop.py','src/files.cjs','src/files-page.js','ui/index.html','ui/style.css','ui/files.css','ui/app.js','ui/plex-latin.woff2','ui/LICENSE-IBM-Plex.txt'];
for(const file of files) assert.ok(asar.extractFile(archive,file).equals(fs.readFileSync(path.join(root,file))),file+' differs from packaged version');
console.log(JSON.stringify({source_matches_packaged_files:files.length,archive},null,2));
