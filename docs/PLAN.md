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
- **Next: M4** (learn).
- Open from M3:
  - Charts are 4/4 only. 3/4 songs (and so a waltz style) need meter support in the engine.
  - My songs live in localStorage. Moving them to IndexedDB is planned for M4, together with progress.
  - MIDI import reads only chords. Keeping the melody for Learn is M4 work.
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
- MIDI input: the sustain pedal is not handled yet, and drum-pad (channel 10) input is treated as notes.

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
  midzer-native/   # (M5) midir + cpal host for Tauri desktop / mobile
packages/
  protocol/        # TS types for UI<->engine messages (generated from Rust via ts-rs/specta)
  audio-web/       # AudioWorklet loader, Web MIDI → worklet forwarding, QWERTY input
apps/
  web/             # React + TS + Vite UI, Zustand state, PixiJS for visuals
  desktop/         # (M5) Tauri 2 shell around apps/web + midzer-native
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

### mp3 → stems → MIDI (M6)
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
  4. **Responsive check:** a laptop and a tablet/phone layout of the Jam screen, so mobile (M7) isn't designed into a corner.
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
- **M7 — Mobile:**
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
- **Performance targets:**
  - Under 30 ms perceived MIDI input→sound latency on Chrome/Linux.
  - No audio dropouts while the visuals run at 60 fps, verified with the Chrome Performance panel and the worklet's underrun counter.
