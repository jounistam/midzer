// Renders the band offline through the release wasm build to a WAV file, for
// listening checks without a browser:
//   node scripts/render-demo.mjs [out.wav] [bpm] [style index]
import { readFileSync, writeFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = join(dirname(fileURLToPath(import.meta.url)), '..');
const out = process.argv[2] ?? join(root, 'demo.wav');
const bpm = Number(process.argv[3] ?? 100);
const style = Number(process.argv[4] ?? 0);
const SR = 48_000;
const BLOCK = 128;

const wasmBytes = readFileSync(join(root, 'apps/web/public/wasm/midzer.wasm'));
const { instance: { exports: x } } = await WebAssembly.instantiate(wasmBytes, {});
const host = x.mz_create(SR);

const font = readFileSync(join(root, 'apps/web/public/soundfonts/GeneralUser-GS.sf2'));
const ptr = x.mz_alloc(font.length);
new Uint8Array(x.memory.buffer, ptr, font.length).set(font);
if (x.mz_load_soundfont(host, ptr, font.length) !== 1) throw new Error('SoundFont failed to load');
x.mz_free(ptr, font.length);

const f32bits = (v) => new Uint32Array(new Float32Array([v]).buffer)[0];
const cmd = (...w) => x.mz_command(host, ...w, 0, 0, 0, 0);
const noteOn = (n) => cmd(1, n, 90);
const noteOff = (n) => cmd(2, n);

// C – G/B – Am – F, twice, with the chord pressed slightly before each bar
// the way a player would.
const progression = [
  [48, 52, 55],
  [47, 50, 55],
  [45, 48, 52],
  [41, 45, 48],
];
const framesPerBar = Math.round(((SR * 60) / bpm) * 4);
const bars = 10;
const totalBlocks = Math.ceil((framesPerBar * bars) / BLOCK);

cmd(10, style); // set style
cmd(7, f32bits(bpm));
cmd(11, 0); // intro, which starts the band

const left = new Float32Array(totalBlocks * BLOCK);
const right = new Float32Array(totalBlocks * BLOCK);
let held = [];
let events = 0;
const chords = [];
for (let b = 0; b < totalBlocks; b++) {
  const frame = b * BLOCK;
  const bar = Math.floor((frame + framesPerBar / 16) / framesPerBar);
  const chord = bar >= 1 && bar < bars - 2 ? progression[(bar - 1) % progression.length] : null;
  // Variation B halfway through, then the ending instead of a hard stop.
  const reaches = (bar) => frame <= framesPerBar * bar && framesPerBar * bar < frame + BLOCK;
  if (reaches(4)) cmd(11, 2);
  if (reaches(7)) cmd(11, 5);
  if (chord && chord !== held.chord) {
    held.forEach(noteOff);
    chord.forEach(noteOn);
    held = [...chord];
    held.chord = chord;
  }
  if (!chord && held.length) {
    held.forEach(noteOff);
    held = [];
  }
  const count = x.mz_process(host, BLOCK);
  events += count;
  const ev = new Uint32Array(x.memory.buffer, x.mz_events_ptr(host), count * 4);
  for (let i = 0; i < ev.length; i += 4) if (ev[i] === 4) chords.push(Math.round((frame + ev[i + 3]) / framesPerBar * 4) / 4);
  left.set(new Float32Array(x.memory.buffer, x.mz_left_ptr(host), BLOCK), frame);
  right.set(new Float32Array(x.memory.buffer, x.mz_right_ptr(host), BLOCK), frame);
}

let peak = 0;
let sumSq = 0;
for (let i = 0; i < left.length; i++) {
  peak = Math.max(peak, Math.abs(left[i]), Math.abs(right[i]));
  sumSq += left[i] ** 2;
}

// 16-bit PCM stereo WAV
const pcm = Buffer.alloc(left.length * 4);
for (let i = 0; i < left.length; i++) {
  pcm.writeInt16LE(Math.round(Math.max(-1, Math.min(1, left[i])) * 32767), i * 4);
  pcm.writeInt16LE(Math.round(Math.max(-1, Math.min(1, right[i])) * 32767), i * 4 + 2);
}
const header = Buffer.alloc(44);
header.write('RIFF', 0);
header.writeUInt32LE(36 + pcm.length, 4);
header.write('WAVEfmt ', 8);
header.writeUInt32LE(16, 16);
header.writeUInt16LE(1, 20);
header.writeUInt16LE(2, 22);
header.writeUInt32LE(SR, 24);
header.writeUInt32LE(SR * 4, 28);
header.writeUInt16LE(4, 32);
header.writeUInt16LE(16, 34);
header.write('data', 36);
header.writeUInt32LE(pcm.length, 40);
writeFileSync(out, Buffer.concat([header, pcm]));

console.log(`wrote ${out}: ${(left.length / SR).toFixed(1)} s at ${bpm} bpm`);
console.log(`events: ${events}, chord changes at bars: ${chords.join(', ')}`);
console.log(`peak ${peak.toFixed(3)}, rms ${Math.sqrt(sumSq / left.length).toFixed(3)}`);
