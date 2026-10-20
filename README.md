# midzer

An interactive backing band: play a MIDI keyboard (or your computer keyboard) and the band follows your chords.

## Layout

| Path | What |
|---|---|
| `crates/midzer-core` | Platform-independent Rust engine: chord recognition, styles, conductor (sections, energy, stop detection), song charts, band scheduler. No I/O, no allocation on the audio path. |
| `crates/midzer-synth` | SoundFont playback of engine events (rustysynth). |
| `crates/midzer-wasm` | Raw C-ABI wasm exports for the AudioWorklet; includes the no-allocation test. |
| `packages/audio-web` | AudioWorklet host, lock-free record rings (SharedArrayBuffer), protocol mirror, Web MIDI. |
| `apps/web` | React + Vite UI (design direction A), PixiJS keyboard, Playwright e2e. `src/songs` holds the song model, chart notation, ChordPro/MIDI import and MIDI export. |

## Development

Requires Rust (with `rustup target add wasm32-unknown-unknown`), Node and pnpm.

```sh
pnpm install
pnpm dev     # downloads the SoundFont once, builds the wasm engine, starts Vite
pnpm test    # cargo test + vitest
pnpm e2e     # Playwright against the production build, using the system Chrome
pnpm build   # production build in apps/web/dist
pnpm demo    # renders the band offline to demo.wav (no browser needed)
```

Computer-keyboard playing: `A W S E D F T G Y H U J K` is one octave from middle C, and Space starts or stops the band.

In Jam, the band follows how hard and how much you play ("Follow me"), or you drive it with the Intro / A / B / C / Fill / Ending pads. When you stop playing for two bars it rests until you come back (Settings can change that to an ending or to keep playing). Settings → Timing → Calibrate measures your latency by tapping along with a click.

In Songs, the band plays a chord chart while you play on top: built-in songs, your own, or ones imported from MIDI or ChordPro files (drop them on the Songs page or paste chords). Click a bar to start there, shift-click to select bars to loop, and use Edit chart to change chords, sections and fills. In both Jam and Songs, everything from Play to Stop is recorded, and "Save last take as MIDI" downloads it with one track per part.

`pnpm demo [out.wav] [bpm] [style]` renders any style (0 Pop, 1 Rock, 2 Funk, 3 Bossa, 4 Ballad, 5 Blues) with an intro, a variation change and the ending.

The dev and preview servers send `Cross-Origin-Opener-Policy: same-origin` and
`Cross-Origin-Embedder-Policy: require-corp` so `SharedArrayBuffer` is available.
Production hosting must send the same headers; without them the app falls back to
`postMessage` between the UI and the audio thread.

Web MIDI needs Chrome, Edge or Firefox.

## Sounds

The built-in instruments come from [GeneralUser GS](https://github.com/mrbumpy409/GeneralUser-GS) (free for any
use; see `apps/web/public/soundfonts/GeneralUser-GS-LICENSE.txt` after fetching). It is downloaded at a pinned commit
and checked by hash; it is not committed to the repository.
