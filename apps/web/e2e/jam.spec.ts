import { expect, test } from '@playwright/test';

const SHOTS = process.env.MIDZER_SHOTS;

test('first run: onboarding to a band that follows a chord', async ({ page }) => {
  const errors: string[] = [];
  page.on('pageerror', (e) => errors.push(e.message));
  page.on('console', (m) => m.type() === 'error' && errors.push(m.text()));
  page.on('response', (r) => r.status() >= 400 && errors.push(`${r.status()} ${r.url()}`));

  await page.goto('/');
  expect(await page.evaluate(() => crossOriginIsolated)).toBe(true);
  await expect(page.getByRole('heading', { name: 'Plug in your keyboard' })).toBeVisible();
  if (SHOTS) await page.screenshot({ path: `${SHOTS}/1-connect.png` });

  await page.getByRole('button', { name: 'Use computer keyboard' }).click();
  await expect(page.getByRole('heading', { name: 'Pick a vibe' })).toBeVisible();
  if (SHOTS) await page.screenshot({ path: `${SHOTS}/2-vibe.png` });
  await page.getByRole('button', { name: 'Continue' }).click();

  await expect(page.getByRole('heading', { name: 'Press Play and hold a chord' })).toBeVisible();
  const play = page.getByRole('button', { name: 'Play', exact: true });
  await expect(play).toBeEnabled({ timeout: 30_000 }); // sounds loaded

  await play.click();
  await expect(page.getByRole('button', { name: 'Stop' })).toBeVisible();
  await expect(page.getByText(/^Bar \d+$/)).toBeVisible();

  // C major on the computer keyboard: A D G = C4 E4 G4.
  for (const key of ['a', 'd', 'g']) await page.keyboard.down(key);
  await expect(page.locator('.chord-name')).toHaveText('C', { timeout: 5_000 });
  await expect(page.getByText('C · E · G')).toBeVisible();
  if (SHOTS) await page.screenshot({ path: `${SHOTS}/3-play.png` });
  for (const key of ['a', 'd', 'g']) await page.keyboard.up(key);

  // A minor: H (A4) + A (C4) + D (E4)
  for (const key of ['h', 'a', 'd']) await page.keyboard.down(key);
  await expect(page.locator('.chord-name')).toHaveText('Am/C', { timeout: 5_000 });
  for (const key of ['h', 'a', 'd']) await page.keyboard.up(key);

  await page.getByRole('button', { name: 'Start jamming' }).click();
  await expect(page.getByRole('button', { name: 'Bass', exact: true })).toBeVisible();
  await page.getByRole('button', { name: 'Bass', exact: true }).click();
  await expect(page.getByRole('button', { name: 'Bass (muted)' })).toBeVisible();
  await page.getByRole('button', { name: 'Faster' }).click();
  await expect(page.locator('.tempo-value input')).toHaveValue('101');
  for (const key of ['s', 'f', 'h']) await page.keyboard.down(key); // D F A
  await expect(page.locator('.chord-name')).toHaveText('Dm', { timeout: 5_000 });

  // Variation C: a fill first, then C from the next bar line.
  const sectionLine = page.locator('.section-line');
  await page.getByRole('button', { name: 'Variation C' }).click();
  await expect(sectionLine).toHaveText(/→ C|Fill/, { timeout: 3_000 });
  await expect(sectionLine).toHaveText('Pop · C', { timeout: 6_000 });
  await expect(page.getByRole('button', { name: 'Follow me' })).toHaveAttribute('aria-pressed', 'false');
  await page.waitForTimeout(400);
  if (SHOTS) await page.screenshot({ path: `${SHOTS}/4-jam.png` });

  // Styles change on the bar line too.
  await page.getByLabel('Style').selectOption({ label: 'Funk · Tight 16ths, slap bass, clav' });
  await expect(sectionLine).toHaveText(/^Funk · /, { timeout: 6_000 });
  for (const key of ['s', 'f', 'h']) await page.keyboard.up(key);

  // Your keys can play other sounds; the choice is remembered.
  await page.getByLabel('Your sound').selectOption({ label: 'Warm pad' });
  for (const key of ['a', 'd', 'g']) await page.keyboard.down(key);
  await page.waitForTimeout(300);
  for (const key of ['a', 'd', 'g']) await page.keyboard.up(key);

  // The ending plays one bar, then the band stops by itself.
  await page.getByRole('button', { name: 'Ending' }).click();
  await expect(page.getByRole('button', { name: 'Play', exact: true })).toBeVisible({ timeout: 10_000 });

  // Timing calibration: tap with the click.
  await page.getByRole('button', { name: 'Settings' }).click();
  await page.getByRole('button', { name: 'Calibrate' }).click();
  await page.getByRole('button', { name: 'Start' }).click();
  const beatMs = 60_000 / 101;
  for (let i = 0; i < 8; i++) {
    await page.waitForTimeout(beatMs);
    await page.keyboard.press('k');
  }
  await expect(page.getByText(/Your taps land|too uneven/)).toBeVisible();
  if (await page.getByRole('button', { name: 'Save', exact: true }).isVisible()) {
    await page.getByRole('button', { name: 'Save', exact: true }).click();
    await expect(page.getByText(/Calibrated: /)).toBeVisible();
  }

  await page.reload();
  await page.getByRole('button', { name: 'Jam' }).click();
  await expect(page.getByLabel('Your sound')).toHaveValue('89');

  expect(errors).toEqual([]);
});
