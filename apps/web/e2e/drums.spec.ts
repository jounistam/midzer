import { expect, test, type Page } from '@playwright/test';

const SHOTS = process.env.MIDZER_SHOTS;

declare global {
  interface Window {
    /** Sends raw MIDI bytes from the fake e-kit. */
    kitSend(bytes: number[]): void;
  }
}

/** Replaces Web MIDI with one fake input, an e-kit the test plays through `kitSend`. */
async function fakeKit(page: Page) {
  await page.addInitScript(() => {
    const input = { id: 'kit', name: 'Fake e-kit', onmidimessage: null as ((e: { data: Uint8Array }) => void) | null };
    const access = {
      inputs: new Map([[input.id, input]]),
      outputs: new Map(),
      addEventListener() {},
      removeEventListener() {},
    };
    Object.defineProperty(navigator, 'requestMIDIAccess', { value: async () => access });
    window.kitSend = (bytes) => input.onmidimessage?.({ data: new Uint8Array(bytes) });
  });
}

test('a drummer: onboarding, computer pads, an e-kit and learning its notes', async ({ page }) => {
  const errors: string[] = [];
  page.on('pageerror', (e) => errors.push(e.message));
  page.on('console', (m) => m.type() === 'error' && errors.push(m.text()));
  await fakeKit(page);

  await page.goto('/');
  await page.getByRole('button', { name: 'I play drums' }).click();
  await expect(page.getByRole('heading', { name: 'Plug in your e-kit' })).toBeVisible();
  await page.getByRole('button', { name: 'Continue' }).click();
  await page.getByRole('button', { name: 'Continue' }).click();

  await expect(page.getByRole('heading', { name: 'Hit your drums' })).toBeVisible();
  await expect(page.getByRole('button', { name: 'Play', exact: true })).toBeEnabled({ timeout: 30_000 });
  const lastHit = page.locator('.kit-last');
  await expect(lastHit).toHaveText('Hit a drum');

  // Computer pads: F is the snare, Shift accents.
  await page.keyboard.press('f');
  await expect(lastHit).toHaveText('Snare · 96');
  await page.keyboard.press('Shift+V');
  await expect(lastHit).toHaveText('Kick · 127');

  // Clicking the middle of the ride hits its bell.
  const kit = page.getByRole('img', { name: 'Drum kit' });
  const box = (await kit.boundingBox())!;
  const scale = Math.min(box.width / 12, box.height / 5);
  const ox = (box.width - 12 * scale) / 2;
  const oy = (box.height - 5 * scale) / 2;
  await page.mouse.click(box.x + ox + 10.4 * scale, box.y + oy + 2.2 * scale);
  await expect(lastHit).toHaveText('Ride bell · 100');

  // The e-kit on channel 10, with General MIDI notes.
  await page.evaluate(() => window.kitSend([0x99, 38, 120]));
  await expect(lastHit).toHaveText('Snare · 120');
  await page.evaluate(() => window.kitSend([0x99, 26, 90])); // not a General MIDI note
  await page.waitForTimeout(300);
  await expect(lastHit).toHaveText('Snare · 120');
  if (SHOTS) await page.screenshot({ path: `${SHOTS}/drums-1-onboarding.png` });

  await page.getByRole('button', { name: 'Start jamming' }).click();
  await expect(page.getByRole('img', { name: 'Drum kit' })).toBeVisible();
  await expect(page.getByRole('img', { name: 'Keyboard' })).toHaveCount(0);
  if (SHOTS) {
    await page.evaluate(() => [[0x99, 36, 127], [0x99, 46, 90], [0x99, 38, 70], [0x99, 51, 110]].forEach(window.kitSend));
    await page.waitForTimeout(60);
    await page.getByRole('img', { name: 'Drum kit' }).screenshot({ path: `${SHOTS}/drums-jam.png` });
  }

  // Learn the kit: its hi-hat edge sends note 26.
  await page.getByRole('button', { name: 'Settings' }).click();
  await expect(page.getByRole('heading', { name: 'Drum kit' })).toBeVisible();
  await page.getByRole('button', { name: 'Learn my kit' }).click();
  const dialog = page.getByRole('dialog', { name: 'Learn my kit' });
  await expect(dialog.getByText('Hit the kick')).toBeVisible();
  await page.evaluate(() => window.kitSend([0x99, 36, 100]));
  await expect(dialog.getByText('Heard note 36')).toBeVisible();
  await dialog.getByRole('button', { name: 'Next' }).click();
  await page.evaluate(() => window.kitSend([0x99, 38, 100]));
  await page.evaluate(() => window.kitSend([0x99, 40, 100]));
  await expect(dialog.getByText('Heard note 38, 40')).toBeVisible();
  await dialog.getByRole('button', { name: 'Next' }).click();
  for (const piece of ['snare rim', 'cross-stick']) {
    await expect(dialog.getByText(`Hit the ${piece}`)).toBeVisible();
    await dialog.getByRole('button', { name: 'Skip' }).click();
  }
  await expect(dialog.getByText('Hit the hi-hat closed')).toBeVisible();
  await dialog.getByRole('button', { name: 'Skip' }).click();
  await page.evaluate(() => window.kitSend([0x99, 26, 100]));
  await dialog.getByRole('button', { name: 'Next' }).click();
  while (await dialog.getByRole('button', { name: 'Skip' }).isVisible()) await dialog.getByRole('button', { name: 'Skip' }).click();
  await expect(dialog.getByText('Learned 3 of 17 pieces: Kick, Snare, Hi-hat open.')).toBeVisible();
  await dialog.getByRole('button', { name: 'Save' }).click();

  await page.evaluate(() => window.kitSend([0x99, 26, 70]));
  await expect(lastHit).toHaveText('Hi-hat open · 70');
  await page.evaluate(() => window.kitSend([0x99, 40, 60])); // learned as the snare
  await expect(lastHit).toHaveText('Snare · 60');
  await page.evaluate(() => window.kitSend([0x99, 42, 60])); // not hit while learning
  await page.waitForTimeout(300);
  await expect(lastHit).toHaveText('Snare · 60');

  // Drums and the learned kit are remembered.
  await page.reload();
  await page.getByRole('button', { name: 'Jam' }).click();
  await expect(page.getByRole('img', { name: 'Drum kit' })).toBeVisible();
  await page.getByRole('button', { name: 'Settings' }).click();
  await expect(page.getByLabel('Note map')).toHaveValue('learned');
  if (SHOTS) await page.screenshot({ path: `${SHOTS}/drums-2-settings.png` });

  expect(errors).toEqual([]);
});

test('a drummer leads the band: count-in, kit controls and a choke', async ({ page }) => {
  const errors: string[] = [];
  page.on('pageerror', (e) => errors.push(e.message));
  page.on('console', (m) => m.type() === 'error' && errors.push(m.text()));
  await fakeKit(page);
  await page.addInitScript(() => {
    // Only on the first load, so a reload keeps what the test changed.
    if (localStorage.getItem('midzer.onboarded')) return;
    localStorage.setItem('midzer.onboarded', '1');
    localStorage.setItem('midzer.prefs', JSON.stringify({ instrument: 'drums' }));
  });

  await page.goto('/');
  await page.getByRole('button', { name: 'Jam' }).click();
  await expect(page.getByRole('button', { name: 'Play', exact: true })).toBeEnabled({ timeout: 30_000 });
  const stage = page.locator('.drum-stage');
  await expect(stage.getByText('Count in on the hi-hat: four even hits')).toBeVisible();
  await expect(stage.locator('.stage-chord .stage-big')).toHaveText('C');

  // The chords come from a progression loop.
  await page.getByLabel('Chords').selectOption({ label: 'Minor turn · vi–IV–I–V in C' });
  await expect(stage.locator('.stage-chord .stage-big')).toHaveText('Am');
  await expect(stage.getByText('Next F')).toBeVisible();
  if (SHOTS) {
    await page.screenshot({ path: `${SHOTS}/drums-3-count-in.png` });
    await page.setViewportSize({ width: 390, height: 844 });
    await page.screenshot({ path: `${SHOTS}/drums-3-phone.png`, fullPage: true });
    await page.setViewportSize({ width: 1440, height: 900 });
  }

  // Four even hi-hats at 120 BPM start the band at that tempo. Timers can
  // run late on a loaded machine, so a ragged count-in is tried again.
  const stop = page.getByRole('button', { name: 'Stop', exact: true });
  for (let attempt = 0; attempt < 3 && !(await stop.isVisible()); attempt++) {
    await page.evaluate(
      () =>
        new Promise<void>((resolve) =>
          [0, 500, 1000, 1500].forEach((t, i) =>
            setTimeout(() => {
              window.kitSend([0x99, 42, 80]);
              if (i === 3) resolve();
            }, t),
          ),
        ),
    );
    await stop.waitFor({ timeout: 2_500 }).catch(() => page.waitForTimeout(2_100));
  }
  await expect(stop).toBeVisible();
  const bpm = Number(await stage.locator('.stage-tempo .stage-big').textContent());
  expect(bpm).toBeGreaterThanOrEqual(115);
  expect(bpm).toBeLessThanOrEqual(125);
  await expect(stage.getByText('BPM · following you')).toBeVisible();

  // Every timekeeping hit moves the timing meter.
  await page.evaluate(
    () =>
      new Promise<void>((resolve) => {
        [36, 42, 38, 42, 36, 42].forEach((note, i) => setTimeout(() => window.kitSend([0x99, note, 100]), i * 250));
        setTimeout(resolve, 1400);
      }),
  );
  await expect(page.getByTestId('timing')).toHaveText(/Rushing|Dragging|In the pocket/, { timeout: 3_000 });
  if (SHOTS) await page.screenshot({ path: `${SHOTS}/drums-3-stage.png` });

  // A foot switch set to "next section" takes the band from A to B.
  await page.getByRole('button', { name: 'Settings' }).click();
  await page.getByRole('button', { name: 'Assign Next section' }).click();
  // The snare plays a drum, so it isn't taken; the switch is.
  await page.evaluate(() => [[0x99, 38, 100], [0xb9, 64, 127]].forEach(window.kitSend));
  await expect(page.getByText('Pedal or switch · CC 64')).toBeVisible();
  await page.keyboard.press('Escape');
  await page.evaluate(() => [[0xb9, 64, 0], [0xb9, 64, 127]].forEach(window.kitSend));
  await expect(page.getByTestId('stage-section')).toHaveText(/^B$/, { timeout: 6_000 });

  // Grabbing the crash stops the band.
  await page.evaluate(() => window.kitSend([0xa9, 49, 127]));
  await expect(page.getByRole('button', { name: 'Play', exact: true })).toBeVisible();

  // The controls are remembered.
  await page.reload();
  await page.getByRole('button', { name: 'Jam' }).click();
  await page.getByRole('button', { name: 'Settings' }).click();
  await expect(page.getByText('Pedal or switch · CC 64')).toBeVisible();

  expect(errors).toEqual([]);
});
