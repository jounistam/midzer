import { expect, test } from '@playwright/test';
import { mkdtempSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

const SHOTS = process.env.MIDZER_SHOTS;

/** A mono 16-bit WAV of a guitar-like A minor strum every second; Chrome loops it as the mic. */
function strumWav(): string {
  const rate = 48_000;
  const seconds = 2;
  const keys = [45, 52, 57, 60, 64]; // A2 E3 A3 C4 E4
  const samples = new Int16Array(rate * seconds);
  for (let i = 0; i < samples.length; i++) {
    const t = (i % rate) / rate;
    let x = 0;
    for (const key of keys) {
      const hz = 440 * 2 ** ((key - 69) / 12);
      for (let h = 1; h <= 5; h++) x += Math.sin(2 * Math.PI * hz * h * (i / rate)) / h;
    }
    samples[i] = Math.round((x / keys.length) * Math.exp(-t * 2) * 0.4 * 32767);
  }
  const header = Buffer.alloc(44);
  header.write('RIFF', 0);
  header.writeUInt32LE(36 + samples.byteLength, 4);
  header.write('WAVEfmt ', 8);
  header.writeUInt32LE(16, 16);
  header.writeUInt16LE(1, 20); // PCM
  header.writeUInt16LE(1, 22); // mono
  header.writeUInt32LE(rate, 24);
  header.writeUInt32LE(rate * 2, 28);
  header.writeUInt16LE(2, 32);
  header.writeUInt16LE(16, 34);
  header.write('data', 36);
  header.writeUInt32LE(samples.byteLength, 40);
  const path = join(mkdtempSync(join(tmpdir(), 'midzer-mic-')), 'strum.wav');
  writeFileSync(path, Buffer.concat([header, Buffer.from(samples.buffer)]));
  return path;
}

test.use({
  launchOptions: {
    args: [
      '--autoplay-policy=no-user-gesture-required',
      '--use-fake-ui-for-media-stream',
      '--use-fake-device-for-media-stream',
      `--use-file-for-fake-audio-capture=${strumWav()}`,
    ],
  },
});

test('mic: a strummed chord sets the band chord and shows on the meter', async ({ page }) => {
  const errors: string[] = [];
  page.on('pageerror', (e) => errors.push(e.message));
  page.on('console', (m) => m.type() === 'error' && errors.push(m.text()));
  await page.addInitScript(() => localStorage.setItem('midzer.onboarded', '1'));

  await page.goto('/');
  await page.getByRole('button', { name: /^Jam/ }).click();
  await expect(page.getByRole('button', { name: 'Play', exact: true })).toBeEnabled({ timeout: 30_000 });

  await page.getByRole('button', { name: 'Settings' }).click();
  const listenFor = page.getByRole('group', { name: 'Listen for' });
  await listenFor.getByRole('button', { name: 'Chords' }).click();
  await expect(listenFor.getByRole('button', { name: 'Chords' })).toHaveAttribute('aria-pressed', 'true');
  await expect(page.getByRole('meter', { name: 'Microphone level' })).toBeVisible();
  await expect
    .poll(() => page.locator('.mic-meter-fill').evaluate((el) => parseFloat((el as HTMLElement).style.width) || 0), {
      timeout: 5_000,
    })
    .toBeGreaterThan(20);
  if (SHOTS) await page.screenshot({ path: `${SHOTS}/12-mic.png` });

  await page.keyboard.press('Escape');
  await expect(page.locator('.chord-name')).toHaveText('Am', { timeout: 5_000 });

  // Turning the mic off stops listening.
  await page.getByRole('button', { name: 'Settings' }).click();
  await listenFor.getByRole('button', { name: 'Off' }).click();
  await expect(page.getByRole('meter', { name: 'Microphone level' })).toBeHidden();
  expect(errors).toEqual([]);
});
