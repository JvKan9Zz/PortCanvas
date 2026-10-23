import {build} from 'esbuild';
import {rm} from 'node:fs/promises';
await rm(new URL('../ui/chunks/',import.meta.url),{recursive:true,force:true});
await rm(new URL('../ui/components.m1.js',import.meta.url),{force:true});
await build({entryPoints:{app:'frontend/main.ts'},bundle:true,outdir:'ui',chunkNames:'chunks/[name]-[hash]',splitting:true,format:'esm',platform:'browser',target:['es2022'],minify:true,sourcemap:false});
