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
  const recording = (bpm, chords, bars) => {
    const rate = 22_050;
    const beat = (60 / bpm) * rate;
    const lead = rate / 2;
    const n = Math.round(lead + bars * 4 * beat + rate);
    const out = new DataView(new ArrayBuffer(44 + n * 2));
    const text = (at, s) => [...s].forEach((c, i) => out.setUint8(at + i, c.charCodeAt(0)));
    text(0, 'RIFF');
    out.setUint32(4, 36 + n * 2, true);
    text(8, 'WAVEfmt ');
    [16, 1, 1, rate, rate * 2, 2, 16].forEach((v, i) => (i === 0 || i === 3 || i === 4 ? out.setUint32 : out.setUint16).call(out, [16, 20, 22, 24, 28, 32, 34][i], v, true));
    text(36, 'data');
    out.setUint32(40, n * 2, true);
    for (let i = 0; i < n; i++) {
      const t = i / rate;
      const pos = (i - lead) / beat;
      let x = 0;
      if (pos >= 0 && pos < bars * 4) {
        for (const k of chords[Math.floor(pos / 4) % chords.length]) {
          const hz = 440 * 2 ** ((k - 69) / 12);
          for (let h = 1; h <= 4; h++) x += 0.6 ** (h - 1) * Math.sin(2 * Math.PI * hz * h * t) * 0.05;
        }
        const since = (pos % 1) * (beat / rate);
        x += (Math.floor(pos) % 4 === 0 ? 0.6 : 0.35) * Math.sin(2 * Math.PI * 60 * since) * Math.exp(-since * 25);
      }
      out.setInt16(44 + i * 2, Math.max(-1, Math.min(1, x)) * 32767, true);
    }
    return out.buffer;
  };
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

    // A recording becomes a song: C | G at 120 BPM with a kick on each beat, through the Songs page.
    const stems = window.__MIDZER_SELFTEST_STEMS === true;
    document.querySelector('[aria-label="Back"]').click();
    await until('home again', () => button('Songs'), 3_000);
    button('Songs').click();
    await until('songs', () => document.querySelector('input[type=file]'), 3_000);
    const input = document.querySelector('input[type=file]');
    const files = new DataTransfer();
    files.items.add(new File([recording(120, [[48, 60, 64, 67], [43, 59, 62, 67]], 8)], 'selftest take.wav', { type: 'audio/wav' }));
    input.files = files.files;
    input.dispatchEvent(new Event('change', { bubbles: true }));
    await until('recording dialog', () => button('Make the song'), 3_000);
    if (stems) {
      button('Separate the instruments').click();
      await sleep(100); // let the choice render before starting
    }
    button('Make the song').click();
    await until('song from the recording', () => /Read from the recording: 120 BPM/.test(document.body.innerText), stems ? 240_000 : 20_000);
    const bars = [...document.querySelectorAll('[aria-label^="Bar "]')].slice(0, 4).map((b) => b.getAttribute('aria-label'));
    log.push(`bars: ${bars.join(' | ')}`);
    if (!/^Bar 1: C\b/.test(bars[0] ?? '') || !/^Bar 2: G\b/.test(bars[1] ?? '')) throw new Error('wrong chords');
    if (stems) await until('transcription', () => button('Save transcription as MIDI'), 2_000);
    done(true);
  } catch (e) {
    log.push(String(e));
    done(false);
  }
})();
