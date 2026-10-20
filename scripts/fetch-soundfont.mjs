// Downloads the GeneralUser GS SoundFont (free for any use, see its LICENSE.txt)
// into the web app's public folder. Pinned to a commit and verified by hash.
import { createHash } from 'node:crypto';
import { existsSync, mkdirSync, writeFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const COMMIT = '97049183643d5fc5a9322a69c5b09efb667c6c3a';
const BASE = `https://raw.githubusercontent.com/mrbumpy409/GeneralUser-GS/${COMMIT}`;
const FILES = [
  {
    src: 'GeneralUser-GS.sf2',
    name: 'GeneralUser-GS.sf2',
    sha256: '9575028c7a1f589f5770fccc8cff2734566af40cd26ed836944e9a5152688cfe',
  },
  {
    src: 'documentation/LICENSE.txt',
    name: 'GeneralUser-GS-LICENSE.txt',
    sha256: '7b32efefdf95ce38a043799f0659853ddc00fbaa14d8c50f0aca16b9b8b405be',
  },
];

const root = join(dirname(fileURLToPath(import.meta.url)), '..');
const destDir = join(root, 'apps/web/public/soundfonts');
mkdirSync(destDir, { recursive: true });

for (const { src, name, sha256 } of FILES) {
  const dest = join(destDir, name);
  if (existsSync(dest)) continue;
  const res = await fetch(`${BASE}/${src}`);
  if (!res.ok) throw new Error(`${src}: HTTP ${res.status}`);
  const bytes = Buffer.from(await res.arrayBuffer());
  const actual = createHash('sha256').update(bytes).digest('hex');
  if (actual !== sha256) throw new Error(`${src}: sha256 mismatch (got ${actual})`);
  writeFileSync(dest, bytes);
  console.log(`fetched ${name} (${(bytes.length / 1e6).toFixed(1)} MB)`);
}
