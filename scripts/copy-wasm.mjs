// Copies the release wasm build into the web app's public folder.
import { copyFileSync, mkdirSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = join(dirname(fileURLToPath(import.meta.url)), '..');
const built = join(root, 'target/wasm32-unknown-unknown/release');
const destDir = join(root, 'apps/web/public/wasm');

mkdirSync(destDir, { recursive: true });
// The engine for the AudioWorklet, and the recording analysis for its worker.
for (const [from, to] of [
  ['midzer_wasm.wasm', 'midzer.wasm'],
  ['midzer_analyze.wasm', 'analyze.wasm'],
]) {
  copyFileSync(join(built, from), join(destDir, to));
  console.log(`copied ${to} → apps/web/public/wasm/`);
}
