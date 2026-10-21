import { expect, test } from '@playwright/test';

/** A WAV of a simple band: a chord per bar with a kick on every beat. */
function recording(bpm: number, chords: number[][], bars: number): Buffer {
  const rate = 22_050;
  const beat = (60 / bpm) * rate;
  const lead = rate / 2;
  const n = Math.round(lead + bars * 4 * beat + rate);
  const pcm = new Int16Array(n);
  for (let i = 0; i < n; i++) {
    const t = i / rate;
    const pos = (i - lead) / beat;
    let x = 0;
    if (pos >= 0 && pos < bars * 4) {
      const keys = chords[Math.floor(pos / 4) % chords.length]!;
      for (const k of keys) {
        const hz = 440 * 2 ** ((k - 69) / 12);
        for (let h = 1; h <= 4; h++) x += 0.6 ** (h - 1) * Math.sin(2 * Math.PI * hz * h * t) * 0.05;
      }
      const sinceBeat = (pos % 1) * (beat / rate);
      const accent = Math.floor(pos) % 4 === 0 ? 0.6 : 0.35;
      x += accent * Math.sin(2 * Math.PI * 60 * sinceBeat) * Math.exp(-sinceBeat * 25);
    }
    pcm[i] = Math.max(-1, Math.min(1, x)) * 32767;
  }
  const header = Buffer.alloc(44);
  header.write('RIFF', 0);
  header.writeUInt32LE(36 + pcm.byteLength, 4);
  header.write('WAVEfmt ', 8);
  header.writeUInt32LE(16, 16);
  header.writeUInt16LE(1, 20);
  header.writeUInt16LE(1, 22);
  header.writeUInt32LE(rate, 24);
  header.writeUInt32LE(rate * 2, 28);
  header.writeUInt16LE(2, 32);
  header.writeUInt16LE(16, 34);
  header.write('data', 36);
  header.writeUInt32LE(pcm.byteLength, 40);
  return Buffer.concat([header, Buffer.from(pcm.buffer)]);
}

test('songs: a recording becomes a chart', async ({ page }) => {
  const errors: string[] = [];
  page.on('pageerror', (e) => errors.push(e.message));
  page.on('console', (m) => m.type() === 'error' && errors.push(m.text()));
  await page.addInitScript(() => localStorage.setItem('midzer.onboarded', '1'));
  await page.goto('/');
  await page.getByRole('button', { name: /^Songs/ }).click();

  const C = [48, 60, 64, 67];
  const Am = [45, 60, 64, 69];
  const F = [41, 60, 65, 69];
  const G = [43, 59, 62, 67];
  const wav = recording(120, [C, Am, F, G], 16);
  await page.getByLabel('Import a recording, MIDI or ChordPro file').setInputFiles({ name: 'band take.wav', mimeType: 'audio/wav', buffer: wav });
  await expect(page.getByRole('dialog', { name: 'Song from a recording' })).toBeVisible();

  await expect(page.getByRole('heading', { name: 'band take' })).toBeVisible({ timeout: 30_000 });
  await expect(page.getByText(/Read from the recording: 120 BPM/)).toBeVisible();
  for (const [bar, chord] of [[1, 'C'], [2, 'Am'], [3, 'F'], [4, 'G'], [5, 'C']] as const) {
    await expect(page.getByRole('button', { name: new RegExp(`^Bar ${bar}: ${chord}($|[ ,])`) })).toBeVisible();
  }
  // It is saved to My songs.
  await page.getByRole('button', { name: 'Back' }).click();
  await expect(page.getByRole('button', { name: /^band take/ })).toBeVisible();
  expect(errors).toEqual([]);
});

test('songs: a drummer plays along with the recording and gets a timing report', async ({ page }) => {
  const errors: string[] = [];
  page.on('pageerror', (e) => errors.push(e.message));
  page.on('console', (m) => m.type() === 'error' && errors.push(m.text()));
  await page.addInitScript(() => {
    if (localStorage.getItem('midzer.onboarded')) return;
    localStorage.setItem('midzer.onboarded', '1');
    localStorage.setItem('midzer.prefs', JSON.stringify({ instrument: 'drums' }));
  });
  await page.goto('/');
  await page.getByRole('button', { name: /^Songs/ }).click();
  const C = [48, 60, 64, 67];
  const F = [41, 60, 65, 69];
  await page.getByLabel('Import a recording, MIDI or ChordPro file').setInputFiles({ name: 'groove.wav', mimeType: 'audio/wav', buffer: recording(120, [C, F], 16) });
  await expect(page.getByRole('heading', { name: 'groove' })).toBeVisible({ timeout: 30_000 });

  // The recording replaces the band; its own tempo and the band's controls go.
  await expect(page.getByText('The recording keeps its own tempo')).toBeVisible({ timeout: 30_000 });
  await expect(page.getByRole('button', { name: 'Recording', pressed: true })).toBeVisible();
  await expect(page.getByText('Energy')).toHaveCount(0);
  await expect(page.getByText(/every hit is timed against the recording/)).toBeVisible();

  await page.getByRole('button', { name: 'Play', exact: true }).click();
  await expect(page.getByText(/^Bar 2 of \d+$/)).toBeVisible({ timeout: 5_000 });
  await expect(page.locator('.song-now .now-chord')).toHaveText(/^(C|F)$/);
  // Kick and snare on the computer pads for a couple of seconds.
  await page.evaluate(
    () =>
      new Promise<void>((resolve) => {
        for (let i = 0; i < 8; i++) {
          setTimeout(() => window.dispatchEvent(new KeyboardEvent('keydown', { key: i % 2 ? 'f' : 'v' })), i * 250);
        }
        setTimeout(resolve, 2200);
      }),
  );
  await expect(page.getByTestId('timing')).toHaveText(/Rushing|Dragging|In the pocket/, { timeout: 3_000 });
  await page.getByRole('button', { name: 'Stop', exact: true }).click();
  const report = page.getByTestId('timing-report');
  await expect(report).toBeVisible();
  await expect(report.getByText(/within 15 ms, 8 hits/)).toBeVisible();
  await expect(report.getByRole('rowheader', { name: 'Kick' })).toBeVisible();
  await expect(report.getByRole('rowheader', { name: 'Snare' })).toBeVisible();
  if (process.env.MIDZER_SHOTS) await page.screenshot({ path: `${process.env.MIDZER_SHOTS}/recording-report.png`, fullPage: true });

  // The band instead, and back: the choice is remembered.
  await page.getByRole('button', { name: 'Band', exact: true }).click();
  await expect(page.getByText('Energy')).toBeVisible();
  await page.getByRole('button', { name: 'Recording', exact: true }).click();

  // The recording is kept with the song for next time.
  await page.reload();
  await page.getByRole('button', { name: /^Songs/ }).click();
  await page.getByRole('button', { name: /^groove/ }).click();
  await expect(page.getByText('The recording keeps its own tempo')).toBeVisible({ timeout: 30_000 });
  expect(errors).toEqual([]);
});
