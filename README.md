# midzer

An interactive backing band: play a MIDI keyboard (or your computer keyboard) and the band follows your chords.

## Layout

| Path | What |
|---|---|
| `crates/midzer-core` | Platform-independent Rust engine: chord recognition, styles, conductor (sections, energy, stop detection), song charts with wait points for Learn, band scheduler. No I/O, no allocation on the audio path. |
| `crates/midzer-synth` | SoundFont playback of engine events (rustysynth). |
| `crates/midzer-wasm` | Raw C-ABI wasm exports for the AudioWorklet; includes the no-allocation test. |
| `crates/midzer-analyze` | Recordings → songs, plain DSP: decoding (mp3, m4a, FLAC, WAV, Ogg), beat tracking, tuning, key, a chord per beat (chroma + HMM), downbeats, sections and a rough melody. Built to wasm for the web app's worker and used natively by the desktop app. |
| `crates/midzer-stems` | Desktop only: stem separation (HT-Demucs) and note transcription (basic-pitch) through ONNX Runtime, feeding cleaner sources into `midzer-analyze`. Models are downloaded on first use. |
| `crates/midzer-native` | Native host for the desktop app: the engine in a cpal audio callback, MIDI through midir, the microphone and live audio. |
| `apps/desktop` | Tauri 2 desktop app: the web UI with the native engine (`src-tauri`). |
| `packages/audio-web` | AudioWorklet host, lock-free record rings (SharedArrayBuffer), protocol mirror, Web MIDI, and the desktop host's client (`native.ts`). |
| `apps/web` | React + Vite UI (design direction A), PixiJS keyboard and falling notes, Playwright e2e. `src/songs` holds the song model, chart and melody notation, ChordPro/MIDI import and MIDI export; `src/learn` the targets, judge, lessons and progress; `src/storage` the IndexedDB stores. |

## Development

Requires Rust (with `rustup target add wasm32-unknown-unknown`), Node and pnpm.

```sh
pnpm install
pnpm dev     # downloads the SoundFont once, builds the wasm engine, starts Vite
pnpm test    # cargo test + vitest
pnpm e2e     # Playwright against the production build, using the system Chrome
pnpm build   # production build in apps/web/dist
pnpm demo    # renders the band offline to demo.wav (no browser needed)
pnpm desktop        # the desktop app (Tauri) in development
pnpm desktop:build  # a desktop installer in target/release/bundle
```

The desktop app and `crates/midzer-native` need system libraries. On Debian/Ubuntu:

```sh
sudo apt install pkg-config build-essential libasound2-dev libwebkit2gtk-4.1-dev libgtk-3-dev \
  libsoup-3.0-dev libjavascriptcoregtk-4.1-dev librsvg2-dev libssl-dev libdbus-1-dev
cargo install tauri-cli --version '^2'
```

They are left out of plain `cargo build`/`cargo test` (see `default-members`), as is `crates/midzer-stems`, whose build
downloads ONNX Runtime; build them with `cargo test -p midzer-native -p midzer-stems` or the scripts above. `MIDZER_SELFTEST=1 target/release/midzer-desktop`
drives the app once, silently, and exits with the result; `MIDZER_SELFTEST=stems` also separates the stems of its test
recording (downloading the models if they aren't there yet).

Computer-keyboard playing: `A W S E D F T G Y H U J K` is one octave from middle C, and Space starts or stops the band.

In Jam, the band follows how hard and how much you play ("Follow me"), or you drive it with the Intro / A / B / C / Fill / Ending pads. When you stop playing for two bars it rests until you come back (Settings can change that to an ending or to keep playing). Settings → Timing → Calibrate measures your latency by tapping along with a click.

In Songs, the band plays a chord chart while you play on top: built-in songs, your own, or ones imported from recordings, MIDI or ChordPro files (drop them on the Songs page or paste chords). A recording (mp3, m4a, FLAC, WAV or Ogg) is read on your device into a chart with its tempo, chords and sections, plus a rough melody for Learn; nothing is uploaded. In the desktop app, "Separate the instruments" first splits it into drums, bass, other instruments and vocals (it downloads 166 MB of models once and takes a few minutes): the chords get cleaner, the melody comes from the voice, the stems are saved as WAV files, and "Save transcription as MIDI" exports the parts. Click a bar to start there, shift-click to select bars to loop, and use Edit chart to change chords, sections and fills. In both Jam and Songs, everything from Play to Stop is recorded, and "Save last take as MIDI" downloads it with one track per part.

Settings → Microphone lets the band listen instead of (or as well as) a keyboard: "Single notes" for singing or a melody line, "Chords" for a strummed guitar, whose chords the band follows. Use headphones if you can; with speakers the mic also hears the band. "Follow my tempo" lets the band speed up and slow down with you, within 15 % of the set tempo.

Play along is for drummers and any song: play it in another browser tab (and share that tab's audio), through an audio input, or, on Linux and in the desktop app, as system audio (a "Monitor" input). midzer finds the beat as it plays, shows it, and times every hit against it about a beat later; after each run it shows the same timing report as a recording does. "This is the 1" and "Half/Twice as fast" fix a beat found wrongly, and "Sync by tapping" (12 taps on the beat) measures how much later you hear the source than midzer does, once per source.

The desktop app runs the same UI with the engine on a native audio thread, and uses MIDI devices directly (desktop webviews have no Web MIDI).

In Learn, notes and chords fall onto the keyboard. With "Wait for me" on, the band stops at each one until you play it; with it off, the band keeps time and each note is graded by how close to the beat it lands. Lessons go from a first chord to melodies and both hands together, and each one opens once the one before has a star. Any song can be practised too, as melody, chords or both, at a slower speed or in a loop. Your songs and Learn progress are kept in the browser's IndexedDB.

`cargo run --release -p midzer-analyze --example analyze -- song.mp3` prints what the analysis reads from a recording, and
`cargo run --release -p midzer-stems --example transcribe -- song.mp3 models-dir [stems-dir]` runs the full desktop pipeline.

`pnpm demo [out.wav] [bpm] [style]` renders any style (0 Pop, 1 Rock, 2 Funk, 3 Bossa, 4 Ballad, 5 Blues) with an intro, a variation change and the ending.

The dev and preview servers send `Cross-Origin-Opener-Policy: same-origin` and
`Cross-Origin-Embedder-Policy: require-corp` so `SharedArrayBuffer` is available.
Production hosting must send the same headers; without them the app falls back to
`postMessage` between the UI and the audio thread.

Web MIDI needs Chrome, Edge or Firefox.

## Models

Stem separation uses [HT-Demucs](https://github.com/facebookresearch/demucs) (MIT) as exported to ONNX by
[adowu/htdemucs-onnx](https://huggingface.co/adowu/htdemucs-onnx) (MIT), and note transcription uses Spotify's
[basic-pitch](https://github.com/spotify/basic-pitch) (Apache-2.0). Both are downloaded at pinned revisions and
checked by hash; they are not committed to the repository.

## Sounds

The built-in instruments come from [GeneralUser GS](https://github.com/mrbumpy409/GeneralUser-GS) (free for any
use; see `apps/web/public/soundfonts/GeneralUser-GS-LICENSE.txt` after fetching). It is downloaded at a pinned commit
and checked by hash; it is not committed to the repository.
