import {build} from 'esbuild';
await build({entryPoints:['frontend/main.ts'],bundle:true,outfile:'ui/app.js',format:'esm',platform:'browser',target:['es2022'],minify:true,sourcemap:false});
if(process.env.WCS_M1_COMPONENT_TEST==='1')await build({entryPoints:['frontend/m1-components.ts'],bundle:true,outfile:'ui/components.m1.js',format:'esm',platform:'browser',target:['es2022'],minify:true});
