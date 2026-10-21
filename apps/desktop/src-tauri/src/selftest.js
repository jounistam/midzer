// Drives the UI once it loads (MIDZER_SELFTEST=1): silent, then reports to the
// `selftest_done` command, which prints the result and exits the app.
(async () => {
  const log = [];
  const done = (ok) => window.__TAURI_INTERNALS__.invoke('selftest_done', { ok, log: log.join('\n') });
  const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
  const until = async (what, test, ms) => {
    for (const end = Date.now() + ms; Date.now() < end; await sleep(50)) if (test()) return log.push(`ok: ${what}`);
    log.push(`page: ${document.body.innerText.slice(0, 300).replace(/\s+/g, ' ')}`);
    throw new Error(`timed out: ${what}`);
  };
  const button = (text) => [...document.querySelectorAll('button')].find((b) => b.textContent.trim().startsWith(text));
  const key = (type, k) => window.dispatchEvent(new KeyboardEvent(type, { key: k, code: `Key${k.toUpperCase()}` }));
  try {
    if (localStorage.getItem('midzer.onboarded') !== '1') {
      localStorage.setItem('midzer.onboarded', '1');
      location.reload();
      return;
    }
    await until('home', () => button('Jam'), 10_000);
    button('Jam').click();
    await until('sounds loaded', () => button('Play') && !button('Play').disabled, 30_000);

    document.querySelector('[aria-label="Settings"]').click();
    await until('settings', () => document.querySelector('.drawer input[type=range]'), 2_000);
    const volume = document.querySelector('.drawer input[type=range]');
    Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value').set.call(volume, '0');
    volume.dispatchEvent(new Event('input', { bubbles: true }));
    log.push(`MIDI in: ${document.querySelector('.drawer .value')?.textContent}`);
    document.querySelector('[aria-label="Close"]').click();

    for (const k of ['a', 'd', 'g']) key('keydown', k);
    await until('chord C from the keyboard', () => document.querySelector('.chord-name')?.textContent === 'C', 3_000);
    for (const k of ['a', 'd', 'g']) key('keyup', k);

    button('Play').click();
    await until('band reaches bar 2', () => /Bar [2-9]/.test(document.querySelector('.bar-label')?.textContent ?? ''), 8_000);
    button('Stop').click();
    await until('stopped', () => document.querySelector('.bar-label')?.textContent === 'Stopped', 3_000);
    done(true);
  } catch (e) {
    log.push(String(e));
    done(false);
  }
})();
