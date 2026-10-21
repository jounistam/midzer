# midzer — Plan: an interactive MIDI backing band

## Status (2026-10-21)
- **D0 design:** Figma file https://www.figma.com/design/LNI6PLmXotvCZHeRPNukyC
  - Direction **A · Stage / club** was chosen.
  - Tokens, components and screens for A are done. They are sections on the Directions page, because the Starter plan allows only 3 pages.
  - Open:
    - Jam tablet and phone layouts in direction A (blocked by the Figma MCP call limit)
    - the usability walkthrough with 2–3 people
- **M0 skeleton:** done.
- **M1 first sound:** done.
  - Inputs: Web MIDI and QWERTY/on-screen keyboard. MIDI out is timestamped.
  - Chord recognition and the Pop style (drums/bass/keys/pad), with chords latched on the beat.
  - rustysynth with GeneralUser GS.
  - Onboarding, Home and Jam screens, and a PixiJS keyboard.
  - Part mutes and tap tempo.
  - Tests: Rust unit tests, a no-allocation test and Playwright e2e.
- **M2 a reactive band:** done (2026-10-21).
  - Conductor state machine (`conductor.rs`): Intro, variations A/B/C, Fill, Ending and Rest. Fills start on the next beat; other changes land on the bar line.
  - "Follow me": energy comes from velocity and note density. It scales every part's velocity and picks the variation, with hysteresis, a two-bar minimum and a fill into each change. A manual energy slider turns following off.
  - Stop detection: after two quiet bars the band fills and rests on the pad until the player comes back, or plays the ending, or keeps going (a setting). Held keys count as playing; silence before the first note doesn't.
  - Six styles (Pop, Rock, Funk, Bossa, Ballad, Blues), written in a step-grid notation in `style.rs` and built once at startup. They include shuffle (12-step) grids, two-bar patterns, arpeggios and per-style drum kits. Style changes wait for the bar line.
  - Latency calibration: tap along with a click. Chords played just after a beat (within the latency plus 60 ms) count for that beat instead of the next.
  - UI: section pads, an energy meter/slider, a style picker, a status line, the onboarding vibe picker and settings for idle mode and calibration.
- **M3 songs:** done (2026-10-21).
  - The engine plays song charts (`song.rs`). The UI loads a chart with commands: one chord slot per beat and one section mark per bar, in fixed arrays of up to 512 bars. Chord and section changes stay sample-accurate, and charts can be reloaded while playing.
  - Section marks drive the conductor: intro bars, A/B/C (with a fill into each change), fills, and the ending in an ending section's last bar. A chart without an ending gets one after its last bar.
  - While a song plays, the player's chords don't change the harmony and the band never rests for silence. Energy still follows the player.
  - Loop a range of bars, start from any bar, or jump there at the next bar line while playing. The engine reports the song bar on every downbeat.
  - The song model and a chart notation (`C | Am F | C . G7 .`, with `[Verse]`, `[Fill]` and `%` tags) live in `apps/web/src/songs`. Nine built-in songs: common progressions plus public-domain tunes.
  - Imports:
    - ChordPro, pasted or from a file. It has no rhythm, so each chord gets a bar unless the file has grid lines. Lyrics are kept per bar.
    - MIDI files: harmony per half bar from duration-weighted pitch profiles matched against chord templates. Drums are skipped and the file is read as 4/4 with its first tempo; anything else gets a warning.
  - Chart editor: edit per bar or as text; sections, fills, add/duplicate/delete bars; title, artist and tempo. Songs are saved to My songs in localStorage, and a built-in song is saved as a copy.
  - Every performance from Play to Stop is recorded. "Save last take as MIDI" writes a format-1 file with one track per part, on the band's beat grid, including tempo changes.
  - Tests: Rust song and band tests, song commands in the no-allocation test, Vitest for the parsers, SMF and recorder, and a Playwright songs flow.
- **M4 learn:** done (2026-10-21).
  - Wait mode lives in the engine (`SongWait`): the band stops at a song tick, on a beat or inside one, and holds the harmony (drums and bass stop) until the wait moves. It reports `Hold` events, and the song position now comes on every beat.
  - Learn code is in `apps/web/src/learn`:
    - Targets come from a song's melody, its chords, or both. Chords are asked for where they change and again on every bar, but not in the intro.
    - A song clock built from the heard beat and hold events.
    - The judge grades notes as perfect, good or ok against ±70/140/280 ms windows, after the calibrated tap offset. In wait mode, a target played cleanly is perfect, and one played after a wrong note is good. Chords count in any octave or inversion, and must be struck again for each target. Accuracy, streaks, and 1–3 stars.
    - Falling notes in PixiJS (`NoteLanes`) on the keyboard's key layout. The next keys light up on the keyboard.
  - Practice options: wait mode, speed 50–100 %, loop bars, melody in any octave (on by default without a MIDI device, since the computer keyboard has one octave), and the part to play for any song.
  - A lesson path of ten lessons in three units (chords, first melodies, hands together). Each lesson opens once the one before has a star.
  - Songs can have a melody (`Song.melody`, with a small notation in `songs/melody.ts`). Three public-domain tunes were added with melodies (Mary Had a Little Lamb, Twinkle, Ode to Joy). MIDI import keeps the top line of the highest mostly single-note part as the melody.
  - Storage is IndexedDB (`storage/db.ts`), with a memory fallback. It holds My songs (moved over from localStorage on first load) and Learn progress (best stars, accuracy and streak per lesson or song part).
  - Tests: Rust hold tests, waits in the no-allocation test, Vitest for targets, clock, judge, lanes, lessons, progress, the library migration and MIDI melodies, and a Playwright Learn flow.
- **M5 desktop + mic:** done (2026-10-21).
  - Listening (`listen.rs`, in the engine): the input is low-passed and decimated to ~24 kHz and analysed every 256 samples (~11 ms). "Single notes" uses YIN pitch with a two-hop confirmation and hysteresis; "Chords" uses a 4096-point FFT, chroma from spectral peaks and cosine matching against nine chord qualities, confirmed over three analyses and reported again on each strum. Attacks come from the level jumping over a decaying envelope. A gate (sensitivity) ignores quiet sounds. All buffers are allocated up front; the no-allocation test covers both modes.
  - The mic's notes go out on channel 15: shown on the keyboard and judged by Learn, never played by the synth or sent to MIDI out. Its chords drive the band like played chords, with the mic's delay (≈45/120 ms) added to the late-chord grace.
  - Tempo follow (`Band::onset`, off by default): attacks near the eighth-note grid give a smoothed player tempo plus a phase error; the band moves towards them within ±15 % of the set tempo and reports `Tempo` events. Turning it off goes back to the set tempo.
  - Web: the AudioWorklet takes the mic as its input (`getUserMedia`, no AGC/noise suppression; echo cancellation unless "Headphones" is on). Settings → Microphone has the mode, a level meter with the gate, sensitivity and headphones.
  - `midzer-native`: the engine renders in the cpal output callback in 128-frame blocks; commands, events and mic samples travel through lock-free `crossbeam` queues; SoundFonts are parsed off the audio thread and swapped in. A forwarder thread sends events to the UI with the frame being heard, plays MIDI out (midir) at the time each event is heard, and rescans MIDI devices every second. MIDI input goes straight into the engine.
  - `apps/desktop`: Tauri 2 around the same web build. `native.ts` gives the UI the same `EngineHandle` over Tauri commands and one update channel, plus native MIDI. `MIDZER_SELFTEST=1` drives the app once and reports.
  - Tests: listener tests on synthetic notes and strums, engine mic test, tempo follow tests, event decode round trip, Vitest for the protocol and native helpers, a Playwright mic test with a fake microphone playing a strummed Am, and the desktop self-test.
- **M6 recordings → songs:** done (2026-10-21), in two tiers (the user's choice).
  - Tier 1, everywhere (`crates/midzer-analyze`, plain DSP, no models):
    - Decoding with symphonia (mp3, AAC/M4A, FLAC, WAV, Ogg Vorbis), up to 15 minutes, and title/artist from the tags. Windowed-sinc resampling to 22.05 kHz mono.
    - Beats: spectral flux over 40 log bands, the tempo from the envelope's autocorrelation with a log-normal prior around 115 BPM, and beat times by Ellis-style dynamic programming.
    - Chroma from spectral peaks (8192-point frames) after estimating the tuning, split into treble and bass, averaged per beat.
    - Key from Krumhansl profiles. Chords per beat by an HMM over 12 roots × major/minor/7/m7/maj7 plus no chord, with harmonic templates, a bass bonus, a diatonic bonus and rarity penalties. A first pass finds the downbeat (chord changes plus bass onsets); the second makes changes cheaper on the bar line. Short gaps hold the chord, and a bass clearly on another chord tone gives an inversion.
    - Sections from a checkerboard novelty over bar-level self-similarity plus loudness jumps, preferring 4- and 8-bar boundaries. Repeated segments share a label; labels become A/B/C by loudness. A short unrepeated first or last segment is the intro or ending.
    - A rough melody from the mix: harmonic-sum salience over G3–C6 tracked by Viterbi (an L1 step cost, so O(frames × pitches)), cut into notes and quantized to sixteenths.
    - Web: built to `public/wasm/analyze.wasm` (raw C ABI, like the engine) and run in a worker per file (`songs/analyze.worker.ts`). A 4.5-minute mp3 takes about 5 s and 210 MB in wasm, about 1.7 s natively.
  - Tier 2, desktop only (`crates/midzer-stems`, ONNX Runtime through `ort`, statically linked):
    - Models are downloaded on first use into the app's data folder, at pinned revisions and checked by SHA-256: HT-Demucs with fp16 weights as one ONNX graph (166 MB, MIT) and basic-pitch (230 KB, Apache-2.0).
    - Demucs separates 7.8 s segments with quarter overlaps and cross-fades. Graph optimization must stay off, and so must the CPU arena and memory patterns: with them, loading alone took over 6 GB and memory kept growing (an earlier test run took the machine down). Without them it peaks around 1 GB.
    - basic-pitch runs on the bass, other and vocal stems; its note creation (inferred onsets, the melodia trick with a heap) is ported to Rust. Drum hits come from band onsets in the drum stem (kick, snare, hi-hat).
    - These feed tier 1: chords from bass+other, the melody from the voice's top line, and the parts as tracks on the beat grid. The stems are saved as WAV files. A 4.5-minute song takes about 2¾ minutes on this laptop's CPU, peaking at 2.1 GB.
  - UI: drop or pick a recording on the Songs page. The dialog says it stays on the device and to use recordings you have the right to use. The desktop app offers Quick or "Separate the instruments", and the analysis shows its step and progress and can be cancelled. The result opens as a song (`source: 'audio'`) with notes on tempo, key and where the melody came from, and is saved to My songs. "Save transcription as MIDI" exports the parts.
  - Tests: unit tests for the resampler, STFT peaks, beat tracking, tuning/chroma, chords, downbeat, sections, melody, basic-pitch note creation and drums. An end-to-end Rust test renders the band itself (three styles and tempos) through the SoundFont and reads back the tempo, first downbeat and chords. Vitest covers the conversion and MIDI export, and a Playwright test imports a synthetic WAV. The desktop self-test imports one too: Quick by default, with stem separation under `MIDZER_SELFTEST=stems` (21 s, 1.5 GB peak).
- **M7a drum input:** done (2026-10-21).
  - Engine (`drums.rs`): 17 pieces, each reported and sounded as its General MIDI key, and a kit map (128 notes → piece) that starts out as General MIDI.
  - Commands: `DrumNote` (raw notes from a drum source), `DrumHit` (pads), `KitNote`, `SetInstrument`, `HiHatPedal`, `DrumChoke` and `SetKitSound`.
  - MIDI channel 10 is always drums, in both instruments. Its note-offs are ignored, polyphonic aftertouch chokes, and CC 4 is the hi-hat pedal. With the instrument set to Drums, notes on every channel go through the kit map.
  - Hits go out on channel 15 (`PLAYER_DRUM_CHANNEL`, 14 from 0) and are released after a fixed 100 ms gate, because kits send their own releases right after the hit. A retrigger releases first; a choke releases at once.
  - Hits count as playing for energy, stop detection and tempo follow. They never form chords.
  - Kit sounds are off by default (the kit's module plays). When on, the synth plays hits on the drum channel, so they use the band's kit for the style. The host routes `SetKitSound` like `SetGain`.
  - MIDI out sends the player's drums only when the player is echoed, on channel 10. The take recorder writes them as their own "Your drums" track on channel 10.
  - Web (`apps/web/src/drums`): piece names and computer keys, the presets (General MIDI, which covers Alesis, Yamaha DTX and most kits by default, and Roland V-Drums with the tom rims and cymbal edges), and the learn wizard (every note heard during a piece's turn plays it; Skip, Back, Clear). The setup is saved in the prefs.
  - UI: onboarding asks "I play keys / drums". The e-kit has its own connect and play steps. Settings has an "I play" switch and a Drum kit section (note map, Learn my kit, Kit sounds). Chord detection is hidden for drums.
  - `KitView` (PixiJS) replaces the keyboard in Jam and Song for drummers: a top-down kit, with rings for the open hi-hat, snare rim and ride bow. Hits light up by velocity and fade. It shows the pedal position and the last hit with its velocity, and pieces can be clicked or touched. Computer pads: V kick, F snare, J/U/N hi-hat closed/open/pedal, T/Y/H/B toms, E/I crashes, K/O ride/bell, R rim, D cross-stick, W splash, P china; Shift accents.
  - Tests: Rust engine, command and synth tests for drums, drums in the no-allocation test, Vitest for the protocol, MIDI translation, kit maps, layout, the learn wizard and the recorder, and a Playwright drummer flow with a fake Web MIDI e-kit (onboarding, pads, channel-10 hits, learning a kit, remembered after reload).
- **Next: M7b** (drummer mode: the drummer leads the band), then M8 (any backing track). Mobile moves to M9.
- Open from M7a:
  - Nobody has tried it with a real e-kit yet. The Roland map comes from its published default note chart and is unverified; the learn wizard is the fallback.
  - The kit's own open/closed hi-hat notes are trusted; the pedal position is only shown, not used.
  - In M7a the band still plays its own drums and waits for chords; muting them and the harmony sources are M7b.
  - Learn and calibration still expect keys. The desktop app wasn't rebuilt here (its GTK build needs pkg-config), but `midzer-native` builds and passes its tests.
- Open from M6:
  - Only checked by eye on a few real songs (folk-rock, EDM pop and metal). Folk-rock and pop came out close; distorted metal is messy. Sevenths are under-detected on purpose. There is no meter detection (4/4 only) and no tempo map: the chart uses the median tempo.
  - The rough melody from the mix is often the wrong line. With stems, the vocal melody is plausible but unverified by ear.
  - Playing along with the original recording, or with the stems (e.g. without vocals), isn't wired up; the stems are only saved.
  - Stem separation runs on the CPU only; no GPU execution providers are configured.
- Open from M5:
  - Nobody has tried the mic with a real guitar or voice yet. The gate, onset and chord thresholds need a hands-on pass, and there is no echo cancellation in the desktop app.
  - Tempo follow needs a hands-on pass too; it is off by default.
  - The desktop app has only been built on Linux. macOS/Windows builds, code signing and a smaller bundled SoundFont are still to do. The SoundFont is fetched through the webview and sent to the native side (32 MB over IPC).
  - The native mic must run at the output's sample rate; there is no resampling.
- Open from M4:
  - Learn screens aren't in the Figma file; the Figma MCP call limit was hit again. They were built straight in direction A.
  - Nobody has tried the lessons on a real MIDI keyboard yet. The timing windows and the any-octave default need a hands-on pass.
  - No "listen first" guide: the band doesn't play the melody. Charts are still 4/4 only.
  - A loop run isn't scored for stars.
- Open from M3:
  - Charts are 4/4 only. 3/4 songs (and so a waltz style) need meter support in the engine.
  - The song screens aren't in the Figma file yet. They were built straight in direction A.
- Open from M2:
  - The styles need a listening pass on real speakers. Nobody has auditioned them yet; only levels and clipping were checked, with `pnpm demo`.
  - The onboarding vibe cards have no audio preview.

## Deviations from the original plan
- No wasm-pack or wasm-bindgen. `midzer-wasm` exposes raw `extern "C"` functions, because the generated glue needs browser features that the AudioWorklet scope lacks.
- rustysynth applies events at its internal block boundaries. That's a constant 32-frame delay (about 0.7 ms) with no jitter.
- Design tokens live in `apps/web/src/theme.ts`, which mirrors the Figma variable names. A test guards that the CSS only uses defined tokens.

## Known gaps
- The SoundFont is 32 MB and gets parsed on the audio thread. Load it before playback, and replace it with a smaller curated set later. Its sample provenance needs review before any commercial release.
- There's no MIDI-out lookahead. The external sound lines up with the internal audio only within that audio's output latency.
- MIDI input: the sustain pedal is not handled yet.

## Context
The repo is empty (no commits yet). The goal is a tool where a musician or hobbyist plays along with a "backing band" that reacts to what they play. It needs:
- a simple first-run experience
- a training mode
- rich, smooth visuals eventually
- later, turning an mp3 into stems and then into MIDI tracks, so the band can play existing songs.

Decisions from the user:
- **Platform:** web first, then desktop, then **mobile eventually**.
- **Sound:** built-in sounds *and* external MIDI out.
- **Input:** start with MIDI keyboard, MIDI drums/pads and the computer keyboard. Guitar and voice via mic must stay possible.
- **Modes:** free jam with styles (arranger style) *and* song-based playback/training.
- **Performance matters.** The user asked about Kotlin/Wasm vs Rust. Conclusion below: a Rust core with a TypeScript UI.
- **Drummers (2026-10-21):** the piano keyboard doesn't work for them, so they get their own setup before mobile. Mostly e-kits (MIDI). First "the drummer leads the band", then playing along with any backing track, with its tempo and beat found by the app so the playing can be judged.

## Architecture decision: Rust core + TypeScript UI
- **What needs raw performance is the real-time audio thread** (synth, mixing, effects, sample-accurate scheduling). It has hard ~3 ms deadlines and can't tolerate garbage-collection pauses. Rust compiles to WASM with no garbage collector. Kotlin/Wasm uses WasmGC, so it is garbage-collected, which makes it a poor fit for the audio thread.
- **Visuals aren't a language problem, they're a rendering-tech problem.** Smooth, rich visuals come from the GPU (WebGL/WebGPU), and TypeScript drives it fine. Many web games do exactly that. Two rules:
  - Draw the animated parts (falling notes, keyboard, visualizers) with **PixiJS (WebGL/WebGPU)**, not DOM elements.
  - **Keep audio off the main thread.** A heavy animation frame must never cause an audio glitch, and with audio in an AudioWorklet it can't.
- **Why decide now, not later:**
  - Porting a pure TS engine to Rust later is a moderate job, roughly a rewrite of the engine with the tests as its spec.
  - What's really expensive to change later is *where* things run: audio thread vs main thread, and the message protocol between them.
  - Mobile settles it. iOS WKWebView and Android WebView don't reliably support Web MIDI, so mobile needs native MIDI and audio. A Rust core runs natively on all of them; a TS engine would have to be rewritten.
  - So starting with Rust avoids a guaranteed later port.
- **Why not Kotlin Multiplatform:** it would share well with Android, but its garbage collector rules it out for the audio thread, its Web Audio/MIDI ecosystem is thin, and on iOS it would still need native audio code.

---

## 1. How it works (user's view)
Three big buttons on the home screen: **Jam**, **Songs**, **Learn**.

**First run (under 60 seconds to sound):**
1. The app detects the MIDI keyboard. If there is none, it offers the computer or on-screen keyboard.
2. Pick a vibe (Pop, Rock, Funk, Bossa, Ballad, Blues), each with an audio preview.
3. Press **Play** and hold a chord. The band follows.

**Jam:**
- Chord input via a left-hand split, the whole keyboard, or one-finger chords for beginners.
- Controls: Intro / Var A–B–C / Fill / Ending, an Energy slider, Tempo with tap tempo.

**Songs:**
- Sources: built-in chord charts, MIDI file import, ChordPro, and later mp3 conversion.
- The band plays the arrangement while the user plays on top.

**Learn:**
- Falling notes or a highlighted keyboard, chord names and diagrams.
- **Wait mode:** the band holds until the right chord or note is played.
- Loop and slow-down.
- Gentle scoring with streaks, and a lesson path.

**How the band reacts to the player:**

| User does | Band reacts |
|---|---|
| Plays a chord | Re-voices bass/keys/pad. The change is latched on the beat, or immediately in "fast follow". |
| Plays harder / denser | Energy goes up: more drum layers, busier bass, variation B/C. |
| Plays softly / sparsely | Band thins out. |
| Stops playing | Fill, then a break/hold, or ends after N bars. |
| Plays drum pads | Triggers fills or sections, or the band follows the player's groove. |
| Speeds up / slows down | Optional tempo follow. Off by default. |

---

## 2. Technology

### Repo layout (Cargo workspace + pnpm workspace)
```
crates/
  midzer-core/     # pure Rust, no I/O: theory, chord recognizer, analyzer,
                   # conductor, style/pattern engine, song model, scheduler
  midzer-synth/    # SF2/SF3 playback (rustysynth) + mixer/effects (fundsp or hand-rolled)
  midzer-wasm/     # wasm-bindgen bindings; runs core+synth inside AudioWorklet
  midzer-native/   # midir + cpal host for Tauri desktop / mobile (M5)
packages/
  protocol/        # TS types for UI<->engine messages (generated from Rust via ts-rs/specta)
  audio-web/       # AudioWorklet loader, Web MIDI → worklet forwarding, QWERTY input
apps/
  web/             # React + TS + Vite UI, Zustand state, PixiJS for visuals
  desktop/         # Tauri 2 shell around apps/web + midzer-native (M5)
services/
  transcribe/      # (M6) optional Python service if native ONNX path is not enough
```

### Runtime model (web)
```
Main thread (React UI + PixiJS)
  Web MIDI input ──► postMessage/SharedArrayBuffer ring ──►┐
  UI commands (play, style, energy) ──────────────────────►│
                                                           ▼
AudioWorklet thread:  midzer-wasm
  InputQueue → Analyzer (chords, energy, onsets) → Conductor
  → Part players (drums/bass/keys/pad) → sample-accurate event schedule
  → midzer-synth renders audio  ──► speakers
  → outgoing MIDI events + state snapshots ──► ring buffer ──► main thread
                                                  ├► Web MIDI out (to DAW/hardware)
                                                  └► UI/visuals (chord, beat, notes)
```
- **Engine and synth run on the audio thread**, so timing is sample-accurate and independent of UI load.
- **Message passing uses lock-free ring buffers** over `SharedArrayBuffer` (e.g. the `ringbuf.js` pattern). This needs cross-origin isolation, so serve the COOP/COEP headers (the dev server and hosting must set them). `postMessage` is the fallback.
- **MIDI out:** the worklet emits events with timestamps, and the main thread sends them with `output.send(data, timestamp)`. Web MIDI's own scheduling keeps them tight.
- **Input is an abstraction from day one.** MIDI, QWERTY, on-screen and (later) mic all produce the same `MusicalEvent` stream into the core.

### Key libraries
- **Rust crates:**
  - `rustysynth` for SoundFont synthesis
  - `fundsp` (optional) for effects such as reverb/chorus
  - `serde` for serialization
  - `ts-rs` or `specta` to generate the TS types
  - `wasm-bindgen`
  - later: `midir`, `cpal`, `ort` (ONNX Runtime)
- **Web:**
  - React, Vite, Zustand
  - **PixiJS** for the GPU-rendered keyboard, falling notes and visualizers
  - `@tonejs/midi` for MIDI file import/export on the UI side
  - `idb` for IndexedDB persistence
- **Sounds:** a free GM bank (*GeneralUser GS*), plus a few curated high-quality SF2s (piano, e-piano, bass, drums), lazy-loaded.
- **Browsers:** Web MIDI works in Chrome, Edge and Firefox, not Safari. Show a friendly message pointing to Chrome or the desktop app.

### Engine content
- **Style format:** your own JSON/RON. Each part has patterns per section (Intro, A, B, C, Fill, Ending) written against a reference chord, plus re-targeting/voice-leading rules (like arranger "CTAB" rules). Author patterns in a DAW as MIDI files and convert them with a small `xtask` tool. Don't ship proprietary Yamaha/Korg styles.
- **Song model:** sections, a chord chart per bar, tempo/key/meter, optional melody and part tracks. Jam mode, Songs mode, Learn mode and mp3 conversion all share it.

### Desktop & mobile (later)
- **Tauri 2** covers desktop *and* iOS/Android using the same web UI. `midzer-native` replaces the AudioWorklet host:
  - `midir` for MIDI (CoreMIDI, ALSA, WinMM; check Android MIDI support, which may need a small Kotlin plugin using `android.media.midi`)
  - `cpal` for low-latency audio (Oboe/AAudio on Android, CoreAudio on iOS)
- The UI talks to native code through Tauri commands/events using the same `protocol` types, so the UI doesn't change.
- This also fixes Tauri's missing Web MIDI on Linux and macOS webviews.

### Mic / guitar / voice (M5)
- Capture runs in the same AudioWorklet, so the DSP is in Rust.
- Monophonic pitch with YIN/pYIN. Chords via a chroma-based recognizer.
- For polyphonic notes: basic-pitch through ONNX (`ort` natively, ONNX Runtime Web in the browser).
- Expect around 50–150 ms of latency, so chord latches stay beat-quantized for audio input.

### mp3 → stems → MIDI (M6, done: see Status for what was built)
- **Pipeline:**
  1. **Demucs v4** (htdemucs / htdemucs_6s) for stems
  2. a beat/downbeat tracker (`beat_this`)
  3. **basic-pitch** for bass, keys, guitar and vocal melody
  4. a drum transcription model
  5. chord recognition
  6. quantize to the beat grid, clean up, and output a Song model plus multi-track MIDI
- **Where it runs:**
  - The preferred path is native in desktop through ONNX exports run by `ort`.
  - If a model doesn't export cleanly, fall back to a local Python service (`services/transcribe`, FastAPI).
  - In-browser via WebGPU is possible later, but the models are big and slow, so don't start there.
- The output is a "good starting point". Ship a chord/section editor to fix it.

### Drummers (M7)
- **The core problem:** the band gets its harmony from the player's chords, and a drummer plays none. So for a drummer the harmony comes from a song chart or a progression loop, and the drummer controls what a keyboardist doesn't: **time, feel and the arrangement**.
- **Input:**
  - Mostly MIDI e-kits. A new `Command::DrumHit { piece, velocity }`, so drum notes no longer pass through the chord path (this fixes the "channel 10 is treated as notes" gap).
  - Kit maps live in TS (`apps/web/src/drums/`): General MIDI as the default, presets for Roland TD, Alesis and Yamaha DTX, and a learn wizard ("hit the snare… now the kick"). Pieces: kick, snare (head, rim), hi-hat (closed, open, pedal), toms 1–3, crash, ride (bow, bell), plus spare pads.
  - The hi-hat pedal comes as CC4 (openness) and chokes as polyphonic aftertouch; both are mapped.
  - The e-kit's module makes the drum sound, so midzer doesn't play the hits by default (no doubled sound). "Play kit sounds" is a setting for pad controllers without a module.
  - QWERTY and touch pads for trying it without a kit and for Playwright tests.
  - An acoustic kit through the mic (onsets sorted into kick, snare and hi-hat, as M6 does on the drum stem) is possible later but not planned, because most users have e-kits.
- **The instrument choice:** onboarding and settings ask for Keys, Drums or Guitar/voice. With Drums, a PixiJS kit view (pieces light up with velocity) replaces the keyboard.
- **Harmony sources:** song charts (built-in, imported, or converted in M6), and progression loops: a few 4- or 8-bar progressions per style, stored as short built-in looping songs so they reuse song mode.
- **The band in drummer mode:** its drums are muted by default (the options are muted, a light percussion layer such as shaker or tambourine, or a click). Bass, keys and pad play.
- **The drummer leads:**
  - **Count-in:** the band waits until four evenly spaced hits on a chosen piece (hi-hat or stick clicks by default) give the tempo and phase, then starts on the next 1.
  - **Tempo follow tuned for drums:** onsets are weighted by piece and beat position (kick and snare on the main beats count most, ghost notes and fills barely). It's always on in drummer mode, with a wider range and a faster response than the keyboard follow. It must not chase a fill.
  - **Energy** from velocity and density, as now. An experiment: the cymbal picks the variation (closed hi-hat → A, open hi-hat or ride → B, crash-heavy → C).
  - **Fills:** a burst of toms or snare above the groove's usual density, late in the bar, counts as a fill. The band moves to the next section on the bar line, and a crash on that 1 confirms it.
  - **Control from the kit:** a spare pad, cymbal or foot switch (a MIDI CC) can be set to "next section", "fill" or "ending". A cymbal choke stops the band. The on-screen section pads stay too.
  - **Stopping:** after two quiet bars the existing idle behaviour applies (end, rest or keep going).
- **The display:** the hands and feet are busy, so the screen is read from about 2 m, not clicked. Big chord, section and next-section text, the tempo and whether it's following, and a rushing/dragging meter against the band's grid. A new Jam layout in direction A.
- **Latency:** drummers notice about 10 ms. The target setup is an e-kit with the desktop app (native audio). The browser is fine for trying it out. The tap calibration is reused, tapping on the kit.

### Backing tracks (M8)
- **The goal:** play along with any recording and get judged on time, without midzer's band.
- **Two sources:**
  1. **A file, analysed first.** M6 tier 1 already finds beats and downbeats. Add a **tempo map** (beat times, not just the median tempo) so tracks that drift are judged correctly. midzer plays the audio itself (decoded PCM streamed into the worklet or the native mixer), so every beat's sample time is known exactly. With stems (desktop), the drums can be muted: a drum-less mix to play over (this also wires up the M6 "play along with the stems" gap).
  2. **Any audio, live.** Audio that midzer doesn't play: system audio loopback in the desktop app (a PipeWire/PulseAudio monitor source on Linux, WASAPI loopback on Windows, ScreenCaptureKit on macOS), tab audio through `getDisplayMedia` in Chrome, or a line input.
- **Live beat tracking** (in Rust, next to `listen.rs`, allocation-free): spectral-flux onset strength (from `midzer-analyze`, made causal), tempo from a running autocorrelation of the onset envelope, and the beat phase from a comb filter bank or PLL. It locks after a few bars and reports a confidence. The downbeat comes from the drummer's count-in or a tap, or is guessed from where the kick and bass land.
- **Judging:** each drum hit is matched against the nearest 8th or 16th of the track's grid. Judging can look back: a hit is graded once the beats around it are known, about one beat later. That is fine for the statistics. The live meter just lags by a beat.
  - Per-piece offsets (e.g. "snare 12 ms late on the backbeat"), a histogram, and accuracy over time.
  - Clock alignment: the e-kit's MIDI timestamps and the audio clock are put on one timeline. For live sources, the loopback captures audio before the output latency, so it needs its own calibration (tap along to the track once).
- **Later:** the M6 drum transcription as a drum part to learn (lanes per piece), tempo trainer and gap bars.

---

## 3. Things to think about
- **Latency is the #1 UX risk.**
  - Use `latencyHint: 'interactive'` and timestamp MIDI with `event.timeStamp`.
  - Compensate for `outputLatency`.
  - Provide a tap-along calibration screen.
- **Cross-origin isolation (COOP/COEP)** is needed for `SharedArrayBuffer`. It constrains hosting and third-party embeds, so set it up in M0.
- **WASM in an AudioWorklet** has a few gotchas:
  - Load the module via `addModule` and pass the compiled `WebAssembly.Module` in.
  - No allocation in `process()`: preallocate, and use arena/fixed buffers in Rust.
  - Render in 128-frame quanta.
- **Chord-detection ambiguity:** offer split, whole-keyboard and one-finger modes.
- **Sound quality beats feature count.** A few great styles with great samples come first, so budget time for style authoring.
- **Progressive disclosure in the UI:** defaults everywhere, and an "Advanced" drawer for split point, channels and voicings.
- **Copyright:**
  - mp3 conversion only on the user's own files, processed locally, with no hosting or sharing of transcriptions.
  - Built-in songs: public domain material, or chord progressions only.
- **Licenses:**
  - GeneralUser GS: free to redistribute.
  - Demucs: MIT. basic-pitch: Apache-2.0.
  - madmom models are CC BY-NC-SA (non-commercial), so avoid them if the app will be sold.
- **Two languages.** Rust has a learning curve. Keep the Rust/TS boundary small and typed with generated types, so the UI work stays in plain TS.

---

## 4. Milestones
- **D0 — UI design in Figma** (runs in parallel with M0 and must be done before M1's UI work):
  1. **Flows and low-fi wireframes:**
     - first run / onboarding (device detect → vibe → play)
     - Home (Jam / Songs / Learn)
     - Jam screen (chord display, sections, energy, tempo)
     - Song picker and chord chart
     - Learn screen (falling notes + keyboard, wait mode, results)
     - Settings / Advanced drawer
     - Also mark which elements are GPU-drawn (PixiJS) and which are regular page elements (React DOM).
  2. **Visual direction:** 2–3 mood directions (Stage/club, Friendly/playful, Pro instrument). Show each one applied to the *same* Jam screen and Learn screen, so they're easy to compare. The user picks one, or a blend.
  3. **Design tokens and core components** for the chosen direction: colour, type, spacing, radius, motion timing; buttons, big mode cards, chord badge, section pads, energy slider, keyboard, note lanes. Also: a dark/light decision, touch target sizes for mobile, and accessibility contrast.
  4. **Responsive check:** a laptop and a tablet/phone layout of the Jam screen, so mobile (M9) isn't designed into a corner.
  5. **Quick usability check:** walk 2–3 people, including a non-musician, through the onboarding frames. Fix the confusing parts before coding.
  - **Output:** a Figma file with the final screens and tokens. In M1, the tokens are exported to CSS variables and a small TS theme, so PixiJS and React share them.
- **M0 — Skeleton:**
  - Cargo + pnpm workspaces, Vite + React + TS, `wasm-pack` build wired into Vite.
  - COOP/COEP headers on the dev server.
  - An AudioWorklet running a Rust sine-wave "hello", plus a ring-buffer message channel.
  - Vitest + `cargo test`, and CI.
- **M1 — First sound:**
  - Inputs: Web MIDI and QWERTY/on-screen keyboard.
  - In `midzer-core`: chord recognizer and transport/scheduler.
  - In `midzer-synth`: `rustysynth` with a GM bank.
  - One Pop style (drums/bass/keys), Play/Stop/Tempo, MIDI out.
  - Onboarding wizard, and a PixiJS keyboard showing played and band notes.
- **M2 — A band that reacts:** Conductor state machine, energy from velocity and density, stop detection, 5–6 styles, tap tempo, latency calibration.
- **M3 — Songs:** Song model, built-in progressions, MIDI and ChordPro import, chord chart editor, MIDI export of the performance.
- **M4 — Learn:** falling notes (PixiJS), wait mode, loop and slow-down, scoring, a lesson path, progress saved in IndexedDB.
- **M5 — Desktop + mic:**
  - The `midzer-native` host and a Tauri 2 desktop app.
  - Mic input with pitch and chord detection.
  - Optional tempo follow.
- **M6 — mp3 → MIDI:** the native ONNX pipeline (Python fallback), imported as a Song with an editor.
- **M7 — Drummers lead the band** (see "Drummers (M7)" above):
  1. **M7a drum input:** `DrumHit`, kit maps, presets and the learn wizard, the hi-hat pedal and chokes, the instrument choice in onboarding and settings, the PixiJS kit view, QWERTY/touch pads.
  2. **M7b drummer mode:** band drums muted (or percussion/click), progression loops, count-in start, drum-tuned tempo follow, fill and crash detection driving the conductor, control from spare pads and foot switches, the stage display with the rushing/dragging meter. Figma frames for the display in direction A, if the MCP limit allows.
  - **Later ideas, not planned yet:** Drum Learn (lanes per kit piece, lessons from the styles' own grooves and fills, channel 10 kept in MIDI imports), groove tools (timing per piece, tempo trainer, gap bars), an acoustic kit through the mic.
- **M8 — Any backing track** (see "Backing tracks (M8)" above):
  1. **M8a files:** a tempo map from the analysis, midzer playing the track (optionally without the drum stem), and judging against its beats.
  2. **M8b live audio:** loopback and tab-audio capture, the live beat tracker with confidence, look-back judging, and calibration for the capture path.
- **M9 — Mobile:**
  - Tauri 2 builds for iOS and Android.
  - Native MIDI and audio through `midzer-native`.
  - A touch-optimized UI.

The first steps after approval are **D0 (Figma drafts) and M0 (skeleton) in parallel**. M1 starts once the D0 visual direction is picked.

---

## 5. Verification
- **Rust (`cargo test`):**
  - chord recognizer on note sets, including inversions, slash chords and partial chords
  - snapshot tests (`insta`) of style rendering over fixed progressions
  - Conductor transitions from scripted event streams
  - an offline render of N bars to a WAV for listening checks
- **Real-time safety:** a debug build with an allocation counter in `process()` that must stay at zero after warm-up.
- **Manual MIDI testing on Linux:** a real keyboard, or a virtual port (`sudo modprobe snd-virmidi` + `aconnect`). Check that MIDI out reaches a DAW or `fluidsynth`.
- **UI tests (Playwright):** drive the app through the QWERTY/on-screen input or an injected mock input. Check that the band starts, the chord display updates and onboarding completes.
- **Drummers (M7, M8):**
  - Rust tests from scripted hit streams: count-in tempo and phase, tempo follow through a fill without chasing it, fill detection, choke stop, and drum hits in the no-allocation test.
  - The live beat tracker on the band's own renders and on click tracks with tempo drift: time to lock, phase error, and holding through breaks.
  - Vitest for kit maps and presets, the learn wizard and judging. Playwright for a drummer jam through the QWERTY pads.
  - A hands-on pass with a real e-kit (count-in, follow, fills) before M7 counts as done.
- **Performance targets:**
  - Under 30 ms perceived MIDI input→sound latency on Chrome/Linux.
  - No audio dropouts while the visuals run at 60 fps, verified with the Chrome Performance panel and the worklet's underrun counter.
