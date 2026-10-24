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

## Slice 3 (own plan later): learn a song from live audio
- Capture the live tap to a rolling buffer at 22.05 kHz mono while following a source: ~5 MB/min, capped at 15 min.
- When the song ends (a `LiveForgot` silence, a now-playing track change, or the "Save this song" button), run slice 2's analysis on the buffer in the analyse worker (native thread on desktop).
- Store a Song from it with `source: 'live'` and no recording, plus the fingerprint (slice 4) and a small onset/energy envelope (~100 Hz, 8-bit) so the editor can draw something without audio.
- Only complete songs are offered. A partial capture is offered as "partial" and can be extended the next time it plays.

## Slice 4 (own plan later): recognise songs and follow the stored grid
- Metadata: implement `docs/now-playing-plan.md`. A track id maps to a stored song id.
- Fingerprint: landmark hashes (spectral-peak pairs, Shazam-style) at 11 kHz. They are computed in slice 2/3 analysis and stored in an IndexedDB `fingerprints` store keyed by hash. The listener hashes the live input every ~2 s and votes on (song, time offset). A clear vote (N matching hashes on one offset) means a match **and** the position in the song.
- Song change without metadata: silence, a lost match, a jump in tempo or key, or a sudden novelty in chroma.
- Following: when matched, the engine gets the stored beat map (a new `LiveSong` command: beat times + downbeat). It plays the beats and the 1 from it, locked by the fingerprint offset and refined by onset cross-correlation. The live tracker only watches for drift, a lost match or a skip (seek). All chords are known ahead.

## Slice 5 (own plan later): fix beats and chords
- A timeline editor in `SongScreen` showing the waveform (stored recording) or the onset envelope (live-learned), with beat lines and chord blocks. It lets you:
  - drag or nudge the downbeat;
  - halve or double the tempo;
  - add a tempo change point;
  - tap along to re-grid a selection;
  - drag a chord edge to a beat, and change a chord by picking or typing it.
- Edits are stored on the song and survive "Analyse again" (merge, or ask).
- Corrections to live-learned songs also tune their fingerprint offset.
