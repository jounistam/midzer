import { expect, test } from '@playwright/test';
import { mkdtempSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

const SHOTS = process.env.MIDZER_SHOTS;

/**
 * A mono WAV of four bars at 120 BPM: a kick-like thump on 1 and 3, a snare-like
 * burst on 2 and 4 and a tick on the eighths, over two bars of C and two of G
 * (bright tones with a bass note). Chrome loops it as the audio input.
 */
function grooveWav(): string {
  const rate = 48_000;
  const beat = rate / 2;
  const samples = new Int16Array(beat * 16);
  let seed = 1;
  const noise = () => {
    seed = (seed * 1_664_525 + 1_013_904_223) >>> 0;
    return (seed / 2 ** 32) * 2 - 1;
  };
  for (let i = 0; i < samples.length; i++) {
    const eighth = Math.floor(i / (beat / 2));
    const t = (i % (beat / 2)) / rate;
    let x = 0;
    if (eighth % 2 === 0) {
      const b = (eighth / 2) % 2;
      x = b === 0 ? 0.8 * Math.sin(2 * Math.PI * 60 * t) * Math.exp(-t * 25) : 0.5 * noise() * Math.exp(-t * 60);
    } else {
      x = 0.12 * noise() * Math.exp(-t * 200);
    }
    // C E G over C, then G B D over G, with a few harmonics each.
    const chord = i < beat * 8 ? [48, 60, 64, 67] : [43, 55, 59, 62];
    for (const key of chord) {
      const hz = 440 * 2 ** ((key - 69) / 12);
      for (let h = 1; h <= 4; h++) x += (0.035 / h) * Math.sin((2 * Math.PI * hz * h * i) / rate);
    }
    samples[i] = Math.round(Math.max(-1, Math.min(1, x)) * 32767);
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
  const path = join(mkdtempSync(join(tmpdir(), 'midzer-along-')), 'groove.wav');
  writeFileSync(path, Buffer.concat([header, Buffer.from(samples.buffer)]));
  return path;
}

test.use({
  launchOptions: {
    args: [
      '--autoplay-policy=no-user-gesture-required',
      '--use-fake-ui-for-media-stream',
      '--use-fake-device-for-media-stream',
      `--use-file-for-fake-audio-capture=${grooveWav()}`,
    ],
  },
});

/**
 * Plays the computer keys in the page on every beat the screen shows, for
 * `beats` beats: `keys[beat]` for each beat of the bar, several at once when
 * joined with "+" (a chord). Taps land at a steady distance from the heard
 * beat, as a player's would.
 */
async function drumOnTheBeat(page: import('@playwright/test').Page, keys: string[], beats: number) {
  await page.evaluate(
    ({ keys, beats }) =>
      new Promise<void>((resolve) => {
        const dots = document.querySelector('.stage-beats')!;
        let played = 0;
        const observer = new MutationObserver(() => {
          const active = [...dots.children].findIndex((d) => d.classList.contains('active'));
          if (active < 0 || dots.getAttribute('data-played') === String(active)) return;
          dots.setAttribute('data-played', String(active));
          for (const key of keys[active % keys.length]!.split('+')) window.dispatchEvent(new KeyboardEvent('keydown', { key }));
          if (++played >= beats) {
            observer.disconnect();
            resolve();
          }
        });
        observer.observe(dots, { subtree: true, attributes: true, attributeFilter: ['class'] });
      }),
    { keys, beats },
  );
}

test('play along: follows the beat of an audio input and times drums and keys against it', async ({ page }) => {
  const errors: string[] = [];
  page.on('pageerror', (e) => errors.push(e.message));
  page.on('console', (m) => m.type() === 'error' && errors.push(m.text()));
  await page.addInitScript(() => localStorage.setItem('midzer.onboarded', '1'));

  await page.goto('/');
  await page.getByRole('button', { name: /^Play along/ }).click();
  await expect(page.getByRole('button', { name: 'Play', exact: true })).toBeEnabled({ timeout: 30_000 });
  await expect(page.getByTestId('along-status')).toHaveText('Pick something to play along with');
  await page.getByRole('button', { name: 'Drums', exact: true }).click();

  // The fake input stands in for a line in or a monitor of the system's output.
  await page.getByRole('button', { name: 'Listen' }).click();
  await expect(page.getByTestId('along-source')).toContainText('Listening to');
  await expect(page.getByTestId('along-status')).toHaveText('Found the beat', { timeout: 20_000 });
  await expect(page.getByTestId('along-tempo')).toHaveText('120');
  await expect(page.getByRole('meter', { name: 'Sure of the beat' })).not.toHaveAttribute('aria-valuenow', '0');
  // The listener names the chords as they come round.
  await expect(page.getByTestId('along-chord')).toHaveText(/^(C|G)$/, { timeout: 20_000 });

  // Sync: a tap on every beat for three bars. Tapping on the dots, which follow
  // the sync, it can only show the taps' own lag, not take it away.
  await expect(page.getByTestId('along-offset')).toContainText('Not synced yet');
  await page.getByRole('button', { name: 'Sync by tapping' }).click();
  await expect(page.getByTestId('along-sync')).toContainText('0 of 12');
  await drumOnTheBeat(page, ['v'], 12);
  await expect(page.getByTestId('along-offset')).toContainText(/Heard \d+ ms (after|before) midzer gets it/, { timeout: 5_000 });
  await expect(page.getByTestId('timing-report')).toHaveCount(0);

  // A run: the first hit starts it, kick on 1 and 3, snare on 2 and 4, for four bars.
  await drumOnTheBeat(page, ['v', 'f', 'v', 'f'], 8);
  await expect(page.getByRole('button', { name: 'Stop', exact: true })).toBeVisible();
  // While playing, the accuracy is the big thing on screen.
  await expect(page.getByTestId('hit-percent')).toHaveText(/^\d+%$/, { timeout: 5_000 });
  await drumOnTheBeat(page, ['v', 'f', 'v', 'f'], 8);
  await page.getByRole('button', { name: 'Stop', exact: true }).click();
  const report = page.getByTestId('timing-report');
  await expect(report).toBeVisible();
  await expect(report.getByText('16 hits')).toBeVisible();
  // Steady hits on a steady beat: a small spread, and both parts judged.
  const spread = Number((await report.locator('.report-tile').nth(2).locator('.report-value').textContent())!.replace(/[^\d]/g, ''));
  expect(spread).toBeLessThanOrEqual(15);
  await expect(report.getByRole('row', { name: /^Kick 8 / })).toBeVisible();
  await expect(report.getByRole('row', { name: /^Snare 8 / })).toBeVisible();
  if (SHOTS) await page.screenshot({ path: `${SHOTS}/13-play-along.png` });

  // Keys: the computer keys play notes, chords struck together count once.
  await page.getByRole('button', { name: 'Keys', exact: true }).click();
  await drumOnTheBeat(page, ['a+d+g', 'a', 'a+d+g', 'a'], 8);
  if (SHOTS) await page.screenshot({ path: `${SHOTS}/14-play-along-keys.png` });
  await page.getByRole('button', { name: 'Stop', exact: true }).click();
  await expect(report.getByRole('row', { name: /^Keys 8 / })).toBeVisible();

  // Moving the 1 and halving the tempo; then leaving turns it all off.
  await page.getByRole('button', { name: 'Half as fast' }).click();
  await expect(page.getByTestId('along-tempo')).toHaveText('60', { timeout: 20_000 });
  // Starting over forgets the beat, the chords and the halving; they come back from what is heard.
  await page.getByRole('button', { name: 'Start over' }).click();
  await expect(page.getByTestId('along-chord')).toHaveText('—');
  await expect(page.getByTestId('along-tempo')).toHaveText('—');
  // A fresh lock may read a beat a minute off at first.
  await expect(page.getByTestId('along-tempo')).toHaveText(/^1(19|20|21)$/, { timeout: 20_000 });
  await expect(page.getByTestId('along-chord')).toHaveText(/^(C|G)$/, { timeout: 20_000 });
  await page.getByRole('button', { name: 'Stop listening' }).click();
  await expect(page.getByTestId('along-status')).toHaveText('Pick something to play along with');
  await page.getByRole('button', { name: 'Back' }).click();
  await page.getByRole('button', { name: /^Jam/ }).click();
  await expect(page.getByRole('button', { name: 'Play', exact: true })).toBeEnabled();

  // The sync is remembered for this input.
  const prefs = await page.evaluate(() => JSON.parse(localStorage.getItem('midzer.prefs') ?? '{}'));
  expect(Object.keys(prefs.liveOffsets ?? {})).toHaveLength(1);
  expect(errors).toEqual([]);
});

test('play along on keys: the keys of each chord fall onto the keyboard, by level', async ({ page }) => {
  const errors: string[] = [];
  page.on('pageerror', (e) => errors.push(e.message));
  page.on('console', (m) => m.type() === 'error' && errors.push(m.text()));
  await page.addInitScript(() => localStorage.setItem('midzer.onboarded', '1'));

  await page.goto('/');
  await page.getByRole('button', { name: /^Play along/ }).click();
  await expect(page.getByRole('button', { name: 'Play', exact: true })).toBeEnabled({ timeout: 30_000 });
  await page.getByRole('button', { name: 'Keys', exact: true }).click();
  await expect(page.getByTestId('chord-lanes')).toBeVisible();
  // A beginner starts with one key.
  await expect(page.getByTestId('along-level')).toContainText('One key');
  await expect(page.locator('.along-legend')).not.toContainText('Left hand');

  await page.getByRole('button', { name: 'Listen' }).click();
  await expect(page.getByTestId('along-chord')).toHaveText(/^(C|G)$/, { timeout: 30_000 });
  await page.getByRole('button', { name: '4', exact: true }).click();
  await expect(page.getByTestId('along-level')).toContainText('Both hands');
  await expect(page.locator('.along-legend')).toContainText('Left hand');
  // Once the loop comes round again, the chord coming is known.
  await expect(page.getByTestId('along-chord-trail')).toContainText(/next (C|G) in/, { timeout: 30_000 });
  if (SHOTS) await page.screenshot({ path: `${SHOTS}/15-play-along-chords.png` });

  // Playing: the timing shrinks to make room for the chords.
  await drumOnTheBeat(page, ['a+d+g', 'a', 'a+d+g', 'a'], 8);
  await expect(page.locator('.hit-feedback.compact')).toBeVisible();
  await expect(page.getByTestId('chord-lanes')).toBeVisible();
  if (SHOTS) await page.screenshot({ path: `${SHOTS}/16-play-along-chords-playing.png` });
  await page.getByRole('button', { name: 'Stop', exact: true }).click();

  const prefs = await page.evaluate(() => JSON.parse(localStorage.getItem('midzer.prefs') ?? '{}'));
  expect(prefs.alongLevel).toBe(4);
  expect(errors).toEqual([]);
});

test('play along: a solo driven by tapping one key', async ({ page }) => {
  const errors: string[] = [];
  page.on('pageerror', (e) => errors.push(e.message));
  page.on('console', (m) => m.type() === 'error' && errors.push(m.text()));
  await page.addInitScript(() => localStorage.setItem('midzer.onboarded', '1'));

  await page.goto('/');
  await page.getByRole('button', { name: /^Play along/ }).click();
  await expect(page.getByRole('button', { name: 'Play', exact: true })).toBeEnabled({ timeout: 30_000 });
  await page.getByRole('button', { name: 'Keys', exact: true }).click();
  await page.getByRole('button', { name: 'Solo', exact: true }).click();
  await expect(page.getByTestId('solo-level')).toContainText('One key');
  await expect(page.getByTestId('chord-lanes')).toHaveCount(0);

  await page.getByRole('button', { name: 'Listen' }).click();
  // C then G: the home note is one of them, and it is the key to tap.
  await expect(page.getByTestId('solo-prompt')).toContainText(/^Tap (C|G) /, { timeout: 30_000 });
  await expect(page.getByTestId('solo-feel')).toHaveText('Resting');

  // The space bar is the home note: four taps a beat for a few seconds.
  await page.evaluate(
    () =>
      new Promise<void>((resolve) => {
        let taps = 0;
        const timer = setInterval(() => {
          window.dispatchEvent(new KeyboardEvent('keydown', { key: ' ', code: 'Space' }));
          setTimeout(() => window.dispatchEvent(new KeyboardEvent('keyup', { key: ' ', code: 'Space' })), 40);
          if (++taps >= 24) {
            clearInterval(timer);
            resolve();
          }
        }, 125);
      }),
  );
  await expect(page.getByRole('meter', { name: 'Feel' })).not.toHaveAttribute('aria-valuenow', /^([0-9]|[1-4][0-9])$/);
  await expect(page.getByTestId('solo-feel')).toHaveText(/Burning|On fire/);
  if (SHOTS) await page.screenshot({ path: `${SHOTS}/17-play-along-solo.png` });
  // Left alone, it winds down.
  await expect(page.getByTestId('solo-feel')).toHaveText('Resting', { timeout: 10_000 });
  // Taps aren't timed; the space bar didn't start or stop anything else.
  await expect(page.getByTestId('timing-report')).toHaveCount(0);

  // Level 3: the player's own notes, and how each fits.
  await page.getByRole('group', { name: 'Solo level' }).getByRole('button', { name: '3', exact: true }).click();
  await expect(page.getByTestId('solo-level')).toContainText('Your notes, our groove');
  await expect(page.getByTestId('solo-fit')).toContainText('Your notes show here');
  await page.keyboard.press('a');
  await expect(page.getByTestId('solo-fit')).toContainText(/Chord tone|In the scale|Snapped/);
  await expect(page.getByTestId('solo-fit')).toContainText(/\b1 (chord|scale|snapped)/);

  // Call and response: a note asks for a call, then the player answers it.
  await page.getByRole('group', { name: 'Call and response' }).getByRole('button', { name: 'On', exact: true }).click();
  await expect(page.getByTestId('solo-call')).toContainText('Play to start');
  await expect(page.getByTestId('solo-prompt')).toContainText('play it back');
  await page.keyboard.press('a');
  await expect(page.getByTestId('solo-call')).toContainText('Listen', { timeout: 10_000 });
  await expect(page.getByTestId('solo-call')).toContainText('Your turn', { timeout: 10_000 });
  for (let i = 0; i < 3; i++) await page.keyboard.press('a');
  await expect(page.getByTestId('solo-call-last')).toContainText(/Last answer: \d of \d/, { timeout: 10_000 });
  if (SHOTS) await page.screenshot({ path: `${SHOTS}/18-play-along-call.png` });

  const prefs = await page.evaluate(() => JSON.parse(localStorage.getItem('midzer.prefs') ?? '{}'));
  expect(prefs.alongMode).toBe('solo');
  expect(prefs.soloLevel).toBe(3);
  expect(prefs.soloCall).toBe(true);
  expect(errors).toEqual([]);
});

test('play along: a song heard is learned and kept, without its audio, and recognised again', async ({ page }) => {
  // Half a minute of listening at least, then the analysis, then recognising it.
  test.setTimeout(210_000);
  const errors: string[] = [];
  page.on('pageerror', (e) => errors.push(e.message));
  page.on('console', (m) => m.type() === 'error' && errors.push(m.text()));
  await page.addInitScript(() => localStorage.setItem('midzer.onboarded', '1'));

  await page.goto('/');
  await page.getByRole('button', { name: /^Play along/ }).click();
  await expect(page.getByRole('button', { name: 'Play', exact: true })).toBeEnabled({ timeout: 30_000 });
  await page.getByRole('button', { name: 'Listen' }).click();
  await expect(page.getByTestId('along-status')).toHaveText('Found the beat', { timeout: 20_000 });
  // Shorter songs aren't kept.
  await page.waitForTimeout(32_000);
  await page.getByRole('button', { name: 'Save this song' }).click();
  const learned = page.getByTestId('along-learned');
  await expect(learned).toContainText('Learning the song heard');
  // The input was loud from the first moment: only part of a song.
  await expect(learned.getByRole('button', { name: 'Save', exact: true })).toBeVisible({ timeout: 60_000 });
  await expect(learned).toContainText(/12\d BPM/);
  await expect(learned).toContainText('partial');
  if (SHOTS) await page.screenshot({ path: `${SHOTS}/17-play-along-learned.png` });
  // Dismissing asks first.
  await learned.getByRole('button', { name: 'Dismiss' }).click();
  await learned.getByRole('button', { name: 'Cancel' }).click();
  await expect(learned.getByRole('textbox', { name: 'Song name' })).toHaveValue(/^Heard /);
  await learned.getByRole('button', { name: 'Save', exact: true }).click();
  await expect(learned).toContainText('in My songs');

  // Kept, it is recognised as it goes on, and followed from what was stored.
  const followed = page.getByTestId('along-followed');
  await expect(followed).toContainText(/Following\s*Heard /, { timeout: 30_000 });
  await followed.getByRole('button', { name: 'Not this song' }).click();
  await expect(followed).toBeHidden();
  // Starting over, it is recognised again.
  await page.getByRole('button', { name: 'Start over' }).click();
  await expect(followed).toBeVisible({ timeout: 30_000 });
  if (SHOTS) await page.screenshot({ path: `${SHOTS}/18-play-along-following.png` });

  await page.getByRole('button', { name: 'Stop listening' }).click();
  await page.getByRole('button', { name: 'Back' }).click();
  await page.getByRole('button', { name: /^Songs/ }).click();
  await expect(page.locator('.song-card-title', { hasText: /^Heard / })).toBeVisible();
  const kept = await page.evaluate(
    () =>
      new Promise<{ source: string; beats: number; envelope: number; fingerprint: number }>((resolve, reject) => {
        const req = indexedDB.open('midzer');
        req.onerror = () => reject(req.error);
        req.onsuccess = () => {
          const tx = req.result.transaction(['songs', 'fingerprints', 'recordings']);
          const songs = tx.objectStore('songs').getAll();
          const prints = tx.objectStore('fingerprints').getAll();
          const recordings = tx.objectStore('recordings').count();
          tx.oncomplete = () => {
            const song = songs.result[0].song;
            if (recordings.result !== 0) reject(new Error('audio was kept'));
            resolve({ source: song.source, beats: song.heard.beatTimes.length, envelope: song.heard.envelope.length, fingerprint: prints.result[0].length });
          };
        };
      }),
  );
  expect(kept.source).toBe('live');
  expect(kept.beats).toBeGreaterThan(40);
  expect(kept.envelope).toBeGreaterThan(3000);
  expect(kept.fingerprint).toBeGreaterThan(1000);
  expect(errors).toEqual([]);
});
