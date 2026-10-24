# Accurate stored songs: silence reset → neural offline analysis → learn, recognise, edit

## Context
Live play-along guesses everything from the last 8–16 s. Beat This! and BTC see only a sliding window, and chords come from a loop guess. Nothing is cleared when the music stops:
- the core tracker forgets after 2.5 s below −60 dBFS, but `Live`'s downbeat state, the neural listener (40 s of chords) and the UI chord timeline (30 s) all persist.

The goal is super-accurate beats. The way there is to analyse a song once, with full context, store it, then recognise it and follow the stored grid.

The user chose to build it in slices. This plan details slices 1 and 2; slices 3–5 are outlined and each gets its own plan later.
- Songs are identified by **metadata when available, and a local audio fingerprint otherwise**.
- Songs learned from live audio store **analysis + fingerprint only, no audio**.

Existing pieces this builds on:
- `docs/now-playing-plan.md`: MPRIS/Spotify metadata, `LiveReset`/`LiveSeed`, per-track memory.
- `crates/midzer-analyze`: file → song. It is plain DSP today (`lib.rs:174 analyze`).
- `crates/midzer-hear`: Beat This! and BTC in plain Rust.
- Song storage: IndexedDB `storage/db.ts`, with recordings kept per song id in `songs/recordings.ts`. Songs carry `beatTimes` (`songs/song.ts:68`).
- The chart editor for chords.
- The eval kit in `~/.cache/midzer-eval`: the official full-song Beat This!/BTC reference in `ref/` and `scripts/score.py`. `examples/offline_dump.rs` writes `.beats`/`.lab` for scoring.

---

## Status (2026-10-24)
- Slices 1–3 are built; results and what is open are in `docs/PLAN.md` Status ("Accurate stored songs").
- Changes from the plan:
  - Slice 1: `Live` ignores guides whose beat is before the last start-over, because a network run already under way re-locked the tracker to the old song.
  - Slice 2: downbeats were 0.58 → 0.63 F only. The network's own downbeats score 0.91, but one bar phase for the whole song misplaces the 1 after odd bars (2, 3 or 5 beats) that most of the test songs have. The user chose (2026-10-24) to keep one phase and leave wrong 1s to slice 5's downbeat editing.
  - Slice 3:
    - The user chose (2026-10-24) to offer learned songs with Keep / Dismiss rather than save them by themselves.
    - The fingerprint is computed now (matching stays in slice 4), because live songs keep no audio.
    - Several learned songs can wait to be kept (up to 5).
    - A capture's end includes the ~0.5 s the level takes to fall.
- Measuring slice 2 again: `offline_dump` on `files24.txt` (24 songs, ~20 s each natively, under a 4 GB cap) → `score.py` with the Python in `~/.claude/jobs/05b3b04b/tmp/venv` (it has mir_eval; `~/venv` doesn't).

## Slice 1: reset after silence + a reset button

**Core (`crates/midzer-core`)**
- `tracker.rs:394-403`: when the silence reset fires, set a `lost` flag. Expose it as `BeatTracker::take_lost() -> bool`, which reads and clears it with no allocation.
- `live.rs`:
  - In `listen`, after `tracker.push`, `if self.tracker.take_lost() { self.stop(0, out); self.forget(); out.push(0, EngineEvent::LiveForgot) }`.
  - Add `pub fn reset(&mut self, out)` that does the same, for the button.
- `command.rs`: add `Command::LiveReset`, encoded and decoded next to the live ops at :91-104, :279-283, :367-371, plus the round-trip test at :449. This is the same command as now-playing-plan §1, so build it once.
- `engine.rs:207-222`: `LiveReset => self.live.reset(&mut out)`.
- `event.rs`: add `EngineEvent::LiveForgot` with an op, encode/decode and the round-trip test (:387).

**Listener (`crates/midzer-hear/src/hearing.rs`)**
- In `push`, track the RMS of the resampled input. After 2.5 s below −60 dBFS (the same constants as the tracker, `SILENT_DB`/`SILENT_S`), call `reset()` once per quiet stretch, keeping `origin`/`expect` so the stream continues. Don't run either network while it's quiet.
- This works on web (worker) and desktop (thread) with no extra messaging, and stays in step with the core because both see the same audio.

**Web/desktop plumbing**
- `packages/audio-web/src/protocol.ts`: mirror `liveReset` and `LiveForgot`.
- `engine.ts`: add `resetLive()` on `EngineHandle`. It posts `{type:'reset'}` to the hear worker (the message already exists at :310) and sends `liveReset`.
- `native.ts` + desktop `lib.rs`: a `live_reset` command that bumps `live_generation` (`crates/midzer-native/src/audio.rs:69`; `hear.rs:43` already resets on it) and pushes `LiveReset`.
- `apps/web/src/session.ts`:
  - On `LiveForgot`, or after `resetLive()`: run `liveChords.clear()`, set `liveChord` to empty, clear the hints, and call `resetLive()` from `live.ts:62` for the visuals.
  - Export `resetLiveSource()` for the button.
- `screens/PlayAlong.tsx` `BeatFixes` (:310): add a "Start over" button, always enabled while a source is on, in the Stage/club style.

**Tests**
- tracker/live: a locked click track, then 3 s of zeros, emits `LiveForgot` and the downbeat is forgotten. `LiveReset` does the same mid-song.
- The no-allocation test covers both.
- hearing: loud input then 3 s of zeros leaves `chords` empty and `state` None. Short dips under 2.5 s don't reset.
- Vitest: protocol encode/decode.
- Playwright (rebuild wasm + web first): with a fake source, "Start over" clears the chord display.

---

## Slice 2: the networks over the whole song for imports

**New `crates/midzer-hear/src/offline.rs`**: `pub fn hear_song(mix: &[f32] /*22.05 kHz*/, progress) -> SongHeard { beats: Vec<f64>, downbeats: Vec<f64>, chords: Vec<(f64, f64, Option<Chord>)> }`, in seconds.
- Beats:
  1. Log-mel over the whole file with the existing `Mel`.
  2. Beat This! in chunks of 1500 frames with a 6-frame border, `keep_first` aggregation. This matches `scripts/bt_ref.py` (`split_predict_aggregate(s, 1500, 6, "keep_first")`).
  3. Peak-pick with the existing `peaks` (make it `pub(crate)`). Add parabolic interpolation over the logits, so beats come out sub-frame (finer than 20 ms).
  4. Snap downbeats to the nearest beat, like the official minimal postprocessor.
  5. Do **not** fit a straight grid, so tempo drift is kept.
- Chords:
  1. CQT with the existing `Cqt`.
  2. BTC over 108-frame windows at 50 % overlap, keeping each window's centre.
  3. Run `smooth`.
  4. Merge runs into spans.

**`crates/midzer-analyze`**
- Move `midzer-hear` from dev-dependency to dependency. The dev-dep back-edge is allowed.
- In `analyze` (`lib.rs:174`), the beat/chord stage becomes neural by default:
  - `beat_times` come from `hear_song`.
  - The bar phase is a majority vote of network downbeats mod 4. Warn when the downbeats say 3/4, because songs are 4/4.
  - Per-beat chords are the chord heard longest in each beat, reusing the `chord_over` logic (make it shared), then `chords::hold_short_gaps` and `with_bass`.
  - Key, tuning, sections and melody stay on chroma and DSP as now.
- With stems (desktop), BTC gets the `extras.harmony` signal.
- The DSP path stays as a fallback when there are fewer than `MIN_BEATS` neural beats.
- `beats::frames_per_second` users and `offline_dump` keep working because the output shape (`Analysis`) is unchanged.
- Wasm: the analyze wasm now embeds both models (~10 MB, the same files the hear wasm already has). That is accepted, since analysis is rare.
- Bench a 4-min song with `scripts/wasm_bench.mjs`. Progress is reported per chunk under `Step::Beats`/`Step::Chords`.

**UI**
- `SongScreen.tsx`: add "Analyse again" for songs with a stored recording (`recordings.ts`). It replaces `beatTimes`/chords after a confirm, because hand edits would be lost.

**Verification**
- `offline_dump` on `files24.txt` → `score.py` against `ref/`. Expect beat F1 ≈ the reference (port fidelity) and well above the current DSP numbers (keep the before/after table). Downbeat F1 and chord accuracy (root/majmin) are reported too.
- Ear check: import 3 of the user's mp3s and play along. The click should sit on the beats throughout, including tempo drift.
- `cargo test -p midzer-hear -p midzer-analyze`, Vitest, then the Playwright audio import flow (`e2e/audio.spec.ts`) after a rebuild.
- Cap memory on long runs (ort/heavy-run memory rule).

---

## Slice 3: learn a song from live audio

Where the audio is: the listener (`Hearing`) already resamples the live source to 22.05 kHz mono and sees the silences, so the capture lives there. It stays off unless asked for, so `hear_eval` and the tests are unchanged. The analysis runs where imports run (analyse worker on web, a native thread on desktop), never in the listener.

**Listener (`crates/midzer-hear/src/hearing.rs`)**
- `set_capture(on)`. While on, a `Capture { samples: Vec<i16>, clean_start: bool, clean_end: bool }` collects the stream:
  - It starts when sound is first heard after a reset or a silence, with 0.5 s of pre-roll from `audio`.
  - `clean_start` means at least 0.25 s of quiet came before the sound, so the song was heard from its start. `reset()` zeroes `power`/`quiet`, so a source opened mid-song counts as not clean.
  - Capped at 15 min (40 MB as i16); past that it stops collecting and the end counts as not clean.
- It ends three ways:
  - the silence `forget()`: trailing silence trimmed to 0.5 s, `clean_end = true`;
  - `finish()` (the "Save this song" button, or closing the source): `clean_end = false`. If the music goes on, a new capture starts at once, not clean;
  - `reset()` (Start over, a new source): dropped.
- Ended captures under 30 s are dropped. `take_song() -> Option<Capture>` hands out the last one.
- ffi: `mzh_set_capture`, `mzh_finish`, `mzh_take_song` (returns the length, 0 if none) with `mzh_song_ptr` and `mzh_song_flags` (bit 0 clean start, bit 1 clean end).

**Fingerprint (new `crates/midzer-hear/src/fingerprint.rs`)**
- Landmark hashes as in slice 4: 11.025 kHz, STFT 1024 / hop 256, local spectral peaks, each peak paired with a few later ones. A hash packs f1, f2 − f1 and dt into a u32, stored with its frame as `[hash, frame]`.
- It is in `midzer-hear` because slice 4 hashes the live input in the listener with the same code, and `midzer-analyze` already depends on it.
- It is computed here, since a live-learned song keeps no audio to compute it from later. Imports and "Analyse again" get it too.
- Slice 4 does the matching and the hash index; here it is only stored.

**Analysis (`crates/midzer-analyze`)**
- `Analysis` gains `envelope: Vec<u8>` (100 Hz, the level in dB mapped to 0–255, for the slice 5 editor) and `fingerprint: Vec<u32>` (flat pairs).
- `analyze_mix(samples_i16_22k, progress)` = `analyze` on the capture, with no title. ffi `mza_analyze_mix(ptr, len)`; `mza_alloc` already gives the bytes.
- Desktop: the hear thread puts a taken capture in `Shared.captured` and sends `UiUpdate::LiveSong { seconds, clean_start, clean_end }`. A `live_song_analyze(on_progress)` command takes it and runs `analyze_mix` on the analysis thread, like `audio_analyze`. `live_song_finish` sets a flag the hear thread turns into `finish()`.

**Plumbing (`packages/audio-web`)**
- `EngineHandle.onLiveSong(listener)` with `LiveSongCapture { seconds, complete, samples?: Int16Array }`. `complete` = clean start and clean end. `samples` is on web only; desktop keeps them natively.
- `finishLiveSong()`. Closing a live source finishes first.
- The hear worker gets `capture`/`finish` messages and posts `{type:'song', samples, flags}` with the buffer transferred.
- Without the listener (no cross-origin isolation) nothing is captured, and the button is hidden.

**App**
- `Song.source` adds `'live'`, and a new `heard?: { beatTimes, seconds, complete, envelope }`. It is kept apart from `backing`, which means there is a recording. Slices 4 and 5 read `backing ?? heard` as the beat map.
- `songFromAnalysis` builds either. `library.ts` sanitises `heard` like `backing`.
- A `fingerprints` store keyed by song id holds `Uint32Array`s for imports and live songs. `DB_VERSION` → 3; the now-playing plan's `tracks` store goes into the same version if it lands first. Deleting a song deletes its fingerprint.
- `analyzeAudio.ts`: `analyzeCapture(capture, options)`, through the worker on web and `live_song_analyze` natively.
- `session.ts`:
  - Capture is on while a source is on.
  - `onLiveSong` analyses one capture at a time; a newer one waits.
  - State `liveLearned: null | {status:'analysing', progress} | {status:'ready', song, complete} | {status:'failed', message}`.
  - `keepLearnedSong()` saves the song and its fingerprint; `dismissLearnedSong()`; `saveLiveSong()` calls `finishLiveSong()`.
  - Title: "Heard <date time>", editable in the editor.
- `PlayAlong.tsx`:
  - "Save this song" next to Start over.
  - A Stage/club card with the progress, then "Learned a song: 3:41 · 118 BPM · A minor", a "partial" tag when not complete, and Keep / Dismiss buttons.
- Extending a partial song when it plays again needs recognition, so it moves to slice 4.

**Tests**
- hearing:
  - A song from silence to silence gives a clean capture of about its length.
  - Opening mid-song gives a not-clean start.
  - `finish()` gives a not-clean end, and the music goes on as a new capture.
  - `reset()` drops the capture; short captures are dropped; capture off collects nothing.
- fingerprint:
  - The same audio, shifted, gives matching hashes at a constant frame offset.
  - Noise gives few matches.
- analyze: `analyze_mix` on the band test signal gives beats, an envelope and a fingerprint.
- Vitest:
  - `songFromAnalysis` with `heard`;
  - library sanitising;
  - the hear message decode.
- Playwright (rebuild first): with the fake source, "Save this song" → the card → Keep → the song is in Songs, with `source: 'live'`.
- Desktop parts are type-checked only, as before.

## Slice 4 (own plan later): recognise songs and follow the stored grid
- Metadata: implement `docs/now-playing-plan.md`. A track id maps to a stored song id.
- Fingerprint: landmark hashes (spectral-peak pairs, Shazam-style) at 11 kHz. They are computed in slice 2/3 analysis and stored in an IndexedDB `fingerprints` store keyed by **song id**, as a `Uint32Array` of hash, frame pairs (`apps/web/src/songs/fingerprints.ts`). Slice 4 builds the hash → (song, frame) index from it in memory at start-up.
  - What exists (slice 3): `midzer_hear::fingerprint(&[f32] at 22.05 kHz) -> Vec<(hash, frame)>` in `crates/midzer-hear/src/fingerprint.rs`.
    - Frames run at `fingerprint::FPS` (≈43.07/s; 11.025 kHz, hop 256), from the start of the analysed audio. For imports that is the file's start; for live songs it is the capture's start, which is also where `Song.heard.beatTimes` count from.
    - Hash = f1 << 13 | (df + 64) << 6 | dt.
    - The listener can hash a rolling window of its 22.05 kHz `audio` with the same function.
    - Checked on real songs: a 10 s noisy excerpt gives ~500 votes on the right offset against ~10 for the next best (`best_offset` in the fingerprint test shows the voting).
  - Beat maps: `Song.backing` (with a recording) or `Song.heard` (live-learned; `complete`, `envelope`, `heardAt`); read `backing ?? heard`.
  - `Capture.from` is the engine frame of a capture's first sample, if following ever needs it.
  - Partial live songs (`heard.complete === false`) should be extended when recognised again (moved here from slice 3). The listener hashes the live input every ~2 s and votes on (song, time offset). A clear vote (N matching hashes on one offset) means a match **and** the position in the song.
- Song change without metadata: silence, a lost match, a jump in tempo or key, or a sudden novelty in chroma.
- Following: when matched, the engine gets the stored beat map (a new `LiveSong` command: beat times + downbeat). It plays the beats and the 1 from it, locked by the fingerprint offset and refined by onset cross-correlation. The live tracker only watches for drift, a lost match or a skip (seek). All chords are known ahead.

## Slice 5 (own plan later): fix beats and chords
- A timeline editor in `SongScreen` showing the waveform (stored recording) or the onset envelope (live-learned), with beat lines and chord blocks. It lets you:
  - drag or nudge the downbeat;
  - halve or double the tempo;
  - add a tempo change point;
  - tap along to re-grid a selection;
  - drag a chord edge to a beat, and change a chord by picking or typing it.
- The live-learned envelope is `Song.heard.envelope`: a `Uint8Array`, 100 frames a second, 0–255 for −60–0 dBFS (`envelope` in `crates/midzer-analyze/src/lib.rs`).
- The bar phase is one per song (the user's choice for slice 2's open question): fixing wrong 1s by hand belongs here.
- Edits are stored on the song and survive "Analyse again" (merge, or ask).
- Corrections to live-learned songs also tune their fingerprint offset.
