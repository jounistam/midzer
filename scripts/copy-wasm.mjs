// Copies the release wasm build into the web app's public folder.
import { copyFileSync, mkdirSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = join(dirname(fileURLToPath(import.meta.url)), '..');
const src = join(root, 'target/wasm32-unknown-unknown/release/midzer_wasm.wasm');
const destDir = join(root, 'apps/web/public/wasm');

mkdirSync(destDir, { recursive: true });
copyFileSync(src, join(destDir, 'midzer.wasm'));
console.log('copied midzer.wasm → apps/web/public/wasm/');
