import { readFileSync } from 'node:fs';
import { expect, test } from '@playwright/test';
import { readSmf, tempoEvent, writeSmf } from '../src/songs/smf';

const SHOTS = process.env.MIDZER_SHOTS;

test('songs: play a chart, jump, loop, edit, save, import and export', async ({ page }) => {
  const errors: string[] = [];
  page.on('pageerror', (e) => errors.push(e.message));
  page.on('console', (m) => m.type() === 'error' && errors.push(m.text()));
  page.on('dialog', (d) => void d.accept());
  await page.addInitScript(() => localStorage.setItem('midzer.onboarded', '1'));

  await page.goto('/');
  await page.getByRole('button', { name: /^Songs/ }).click();
  await expect(page.getByRole('heading', { name: 'Songs', exact: true })).toBeVisible();
  if (SHOTS) await page.screenshot({ path: `${SHOTS}/5-songs.png` });

  await page.getByRole('button', { name: /Four Chord Anthem/ }).click();
  await expect(page.getByRole('heading', { name: 'Four Chord Anthem' })).toBeVisible();
  const now = page.locator('.now-chord');
  await expect(now).toHaveText('C');
  await expect(page.locator('.next-name')).toHaveText('G');

  const play = page.getByRole('button', { name: 'Play', exact: true });
  await expect(play).toBeEnabled({ timeout: 30_000 });
  await play.click();
  // 100 BPM: a bar is 2.4 s. Bar 1 is C, bar 2 is G.
  await expect(page.getByText('Bar 1 of 37')).toBeVisible({ timeout: 3_000 });
  await expect(now).toHaveText('C');
  await expect(now).toHaveText('G', { timeout: 4_000 });
  await expect(page.getByText('Bar 2 of 37')).toBeVisible();
  await expect(page.locator('.section-line')).toHaveCount(0);
  if (SHOTS) await page.screenshot({ path: `${SHOTS}/6-song.png` });

  // Clicking a bar jumps there on the next bar line: bar 13 is the chorus, F.
  await page.getByRole('button', { name: /^Bar 13: F/ }).click();
  await expect(page.getByText('Bar 13 of 37')).toBeVisible({ timeout: 4_000 });
  await expect(now).toHaveText('F');

  // Loop bars 13–14 (F | G): the band stays there.
  await page.getByRole('button', { name: /^Bar 14: G/ }).click({ modifiers: ['Shift'] });
  await page.getByRole('button', { name: 'Loop bars 13–14' }).click();
  await expect(page.getByRole('button', { name: 'Looping bars 13–14' })).toBeVisible();
  await expect(page.getByText('Bar 14 of 37')).toBeVisible({ timeout: 4_000 });
  await expect(page.getByText('Bar 13 of 37')).toBeVisible({ timeout: 4_000 });

  // Play along a little, then stop and save the take.
  for (const key of ['a', 'd', 'g']) await page.keyboard.down(key);
  await page.waitForTimeout(300);
  for (const key of ['a', 'd', 'g']) await page.keyboard.up(key);
  await page.getByRole('button', { name: 'Stop' }).click();
  const save = page.getByRole('button', { name: 'Save last take as MIDI' });
  await expect(save).toBeEnabled({ timeout: 3_000 });
  const [download] = await Promise.all([page.waitForEvent('download'), save.click()]);
  expect(download.suggestedFilename()).toMatch(/^Four Chord Anthem .*\.mid$/);
  const take = readSmf(readFileSync(await download.path()));
  expect(take.tracks.map((t) => t.name)).toEqual(expect.arrayContaining(['You', 'Bass', 'Drums']));

  // Edit: bar 1 becomes Dm, bar 2 gets a fill; saving a built-in song makes a copy.
  await page.getByRole('button', { name: /^Looping/ }).click();
  await page.getByRole('button', { name: /^Bar 1: C/ }).click();
  await expect(page.getByText('Starts at bar 1')).toBeVisible();
  await page.getByRole('button', { name: 'Edit chart' }).click();
  const bar1 = page.getByLabel('Chords in bar 1', { exact: true });
  await bar1.fill('Dm');
  await bar1.press('Enter');
  await page.getByLabel('Chords in bar 2', { exact: true }).fill('Hm');
  await page.getByLabel('Chords in bar 2', { exact: true }).press('Enter');
  await expect(page.getByLabel('Chords in bar 2', { exact: true })).toHaveAttribute('aria-invalid', 'true');
  await page.getByLabel('Chords in bar 2', { exact: true }).fill('G');
  await page.getByLabel('Chords in bar 2', { exact: true }).press('Enter');
  await page.getByRole('button', { name: 'Fill', exact: true }).click();
  await page.getByLabel('Title').fill('My Anthem');
  if (SHOTS) await page.screenshot({ path: `${SHOTS}/7-edit.png` });
  await page.getByRole('button', { name: 'Save a copy' }).click();
  await expect(page.getByText('Saved to My songs')).toBeVisible();
  await page.getByRole('button', { name: 'Done' }).click();
  await expect(page.getByRole('heading', { name: 'My Anthem (my version)' })).toBeVisible();
  await expect(now).toHaveText('Dm');

  // The copy is in My songs, and the band follows chords again in Jam.
  await page.getByRole('button', { name: 'Back' }).click();
  await expect(page.getByRole('button', { name: /^My Anthem \(my version\)/ })).toBeVisible();

  // Paste ChordPro.
  await page.getByRole('button', { name: 'Paste chords' }).click();
  await page.getByLabel('Chords', { exact: true }).fill('{title: Pasted}\n{start_of_chorus}\n[Am]La la [F]la [C]la [G]la\n');
  await page.getByRole('button', { name: 'Open song' }).click();
  await expect(page.getByRole('heading', { name: 'Pasted' })).toBeVisible();
  await expect(page.getByRole('button', { name: /^Bar 1: Am/ })).toBeVisible();
  await expect(page.locator('.bar-lyric').first()).toHaveText('La la');

  // Import a MIDI file: C, then F, one bar each.
  await page.getByRole('button', { name: 'Back' }).click();
  const events: number[][] = [];
  [[48, 60, 64, 67], [41, 60, 65, 69]].forEach((keys, i) =>
    keys.forEach((k) => events.push([i * 1920, 0x90, k, 90], [(i + 1) * 1920 - 10, 0x80, k, 0])),
  );
  const midi = writeSmf(480, [{ name: 'Piano', events: [tempoEvent(0, 88), ...events] }]);
  await page.getByLabel('Import a recording, MIDI or ChordPro file').setInputFiles({
    name: 'two_chords.mid',
    mimeType: 'audio/midi',
    buffer: Buffer.from(midi),
  });
  await expect(page.getByRole('heading', { name: 'two chords' })).toBeVisible();
  await expect(page.getByText(/Chords were read from the notes/)).toBeVisible();
  await expect(page.getByRole('button', { name: /^Bar 2: F/ })).toBeVisible();
  await expect(page.locator('.tempo-value')).toContainText('88');

  // Leaving Songs hands the harmony back to the player.
  await page.getByRole('button', { name: 'midzer' }).click();
  await page.getByRole('button', { name: /^Jam/ }).click();
  await page.getByRole('button', { name: 'Play', exact: true }).click();
  for (const key of ['s', 'f', 'h']) await page.keyboard.down(key);
  await expect(page.locator('.chord-name')).toHaveText('Dm', { timeout: 5_000 });
  for (const key of ['s', 'f', 'h']) await page.keyboard.up(key);

  expect(errors).toEqual([]);
});
