import { expect, test } from '@playwright/test';

const SHOTS = process.env.MIDZER_SHOTS;

test('learn: a lesson in wait mode, scored, unlocks the next and is remembered', async ({ page }) => {
  const errors: string[] = [];
  page.on('pageerror', (e) => errors.push(e.message));
  page.on('console', (m) => m.type() === 'error' && errors.push(m.text()));
  await page.addInitScript(() => localStorage.setItem('midzer.onboarded', '1'));

  await page.goto('/');
  await page.getByRole('button', { name: /^Learn/ }).click();
  await expect(page.getByRole('heading', { name: 'Learn', exact: true })).toBeVisible();
  await expect(page.getByRole('button', { name: /^Lesson 2: C, F and G, locked/ })).toBeDisabled();
  if (SHOTS) await page.screenshot({ path: `${SHOTS}/8-learn.png` });

  await page.getByRole('button', { name: 'Start: Your first chord: C' }).click();
  await expect(page.getByRole('heading', { name: 'Your first chord: C' })).toBeVisible();
  await expect(page.locator('.now-chord')).toHaveText('C');
  await expect(page.locator('.target-tones')).toHaveText('C: C E G');
  await expect(page.getByRole('img', { name: 'Falling notes' })).toBeVisible();

  const play = page.getByRole('button', { name: 'Play', exact: true });
  await expect(play).toBeEnabled({ timeout: 30_000 });
  await play.click();

  const waiting = page.getByText('The band waits for you');
  const chord = async () => {
    for (const key of ['a', 'd', 'g']) await page.keyboard.down(key); // C E G
    await page.waitForTimeout(80);
    for (const key of ['a', 'd', 'g']) await page.keyboard.up(key);
  };

  // After the intro bar the band stops at bar 1 and waits for C.
  await expect(waiting).toBeVisible({ timeout: 6_000 });
  if (SHOTS) await page.screenshot({ path: `${SHOTS}/9-lesson.png` });
  await page.keyboard.press('w'); // C#: a wrong note first
  await chord();
  await expect(page.locator('.feedback')).toHaveText('Good');
  await expect(waiting).toBeHidden();

  // Four more bars, each waited for and played cleanly.
  for (let i = 0; i < 4; i++) {
    await expect(waiting).toBeVisible({ timeout: 6_000 });
    await chord();
    await expect(page.locator('.feedback')).toHaveText('Perfect');
  }

  // The band plays the ending and stops; the run is scored.
  const result = page.getByRole('dialog', { name: 'Result' });
  await expect(result).toBeVisible({ timeout: 8_000 });
  await expect(result.getByRole('img', { name: '3 of 3 stars' })).toBeVisible();
  await expect(result.getByText('90% accuracy')).toBeVisible();
  if (SHOTS) await page.screenshot({ path: `${SHOTS}/10-result.png` });
  await expect(result.getByRole('button', { name: 'Next: C, F and G' })).toBeVisible();
  await result.getByRole('button', { name: 'All lessons' }).click();

  const second = page.getByRole('button', { name: /^Lesson 2: C, F and G, 0 of 3 stars/ });
  await expect(second).toBeEnabled();
  await expect(page.getByRole('button', { name: /^Lesson 1: Your first chord: C, 3 of 3 stars/ })).toBeVisible();

  // Progress is kept in IndexedDB.
  await page.reload();
  await page.getByRole('button', { name: /^Learn/ }).click();
  await expect(page.getByRole('button', { name: /^Lesson 2: C, F and G, 0 of 3 stars/ })).toBeEnabled();
  await expect(page.getByRole('button', { name: 'Continue: C, F and G' })).toBeVisible();

  // Any song can be practised; one with a melody offers it, in time with the band.
  await page.getByRole('button', { name: 'Practise Mary Had a Little Lamb: Melody' }).click();
  await expect(page.getByRole('heading', { name: 'Mary Had a Little Lamb' })).toBeVisible();
  await expect(page.locator('.now-chord')).toHaveText('E4');
  await page.getByRole('button', { name: 'Hands together' }).click();
  await expect(page.locator('.now-chord')).toHaveText('C + E4');
  await page.getByRole('button', { name: '70%' }).click();
  await expect(page.getByText('Speed · 67 BPM')).toBeVisible();

  expect(errors).toEqual([]);
});
