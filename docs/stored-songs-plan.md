# Accurate stored songs: silence reset → neural offline analysis → learn, recognise, edit

## Context
Live play-along guesses everything from the last 8–16 s. Beat This! and BTC see only a sliding window, and chords come from a loop guess. Nothing is cleared when the music stops:
- the core tracker forgets after 2.5 s below −60 dBFS, but `Live`'s downbeat state, the neural listener (40 s of chords) and the UI chord timeline (30 s) all persist.

The goal is super-accurate beats. The way there is to analyse a song once, with full context, store it, then recognise it and follow the stored grid.

The user chose to build it in slices. This plan details all five slices; slice 5 was planned after slice 4 was built.
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
- Slices 1–4 and 5a are built; results and what is open are in `docs/PLAN.md` Status ("Accurate stored songs"). 5b (the timeline editor) is next.
- Changes from the plan:
  - Slice 1: `Live` ignores guides whose beat is before the last start-over, because a network run already under way re-locked the tracker to the old song.
  - Slice 2: downbeats were 0.58 → 0.63 F only. The network's own downbeats score 0.91, but one bar phase for the whole song misplaces the 1 after odd bars (2, 3 or 5 beats) that most of the test songs have. The user chose (2026-10-24) to keep one phase and leave wrong 1s to slice 5's downbeat editing.
  - Slice 3:
    - The user chose (2026-10-24) to offer learned songs with Keep / Dismiss rather than save them by themselves.
    - The fingerprint is computed now (matching stays in slice 4), because live songs keep no audio.
    - Several learned songs can wait to be kept (up to 5).
    - A capture's end includes the ~0.5 s the level takes to fall.
  - Slice 4:
    - The grid from a stored map is a straight fit (`fit`) through its last 6 s of beats, not the local period: the maps have stray beats in places (0.45/0.67 s steps in a steady 0.9 s song), which threw the grid off (beats 78 → 86 % on four songs).
    - The 1 while following is voted, as live: the stored 1s count a little (0.3 each) and the network's downbeats from its refining runs fully, because one stored bar phase is wrong after odd bars (stored 1s were right 13–19 % on some songs).
    - Where a song repeats, the earliest of the places that match about as well (80 % of the votes) is taken, so a partial song is followed the longest.
    - A song change cuts the capture at the last landmark of the old song heard, not at the end of its last look-up (which lands ~6 s into the next song).
  - Slice 5a:
    - `bar_lines` takes the network's 1s as they come. The planned check of each odd bar against the 1s after it lost songs whose sections change bar length (runs of 5s and 6s): 0.87 F. Trusting them, passing over only a 1 a beat after the last and filling gaps of two bars or more, gives 0.92, above the network's own 0.91. Tuned on the network's beats dumped once (`sweep.py`-style, outside the repo).
    - `chords.rs` keeps its 4/4 phase: only the DSP fallback uses it, and that stays 4/4. Melody notation for built-ins stays 4/4 too.
    - The drummer's fill window and the count-in stay four beats (the fill detector only acts in Jam).
    - A followed song's 1 is now the vote for how many beats after the stored bar lines the 1s are heard (0 = as stored), not for a place in a bar of four.
    - The bar editor keeps a bar's length; "[3/4] C" in it gives another.
- Measuring slice 4 again: `KNOWN=1 START=60 HEAR_REF=~/.cache/midzer-eval/ref hear_eval $(files24)`, against the same without `KNOWN` (~15 min each, under a 5 GB cap).
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

## Slice 4: recognise songs and follow the stored grid
The user chose (2026-10-24) to recognise by fingerprint only. Now-playing metadata (`docs/now-playing-plan.md`) becomes a slice of its own later.

Where it runs: the listener (`Hearing`) already has the last 16 s of the live source at 22.05 kHz, sees the silences, owns the capture and sends the guides, so recognising and following live there too. That keeps web (worker) and desktop (thread) the same.

Change from the outline: no new `LiveSong` engine command. While following, the listener sends the existing `LiveGuide` built from the stored beat map instead of from its own fit: the stored beat, the local stored period, the stored 1, full confidence. The tracker already moves to a guide's grid and snaps its beats to onsets (`tracker.rs guided_beat`), and the guides keep coming about once a second, so tempo drift in the stored map is followed. The core stays unchanged.

What exists (slice 3): `midzer_hear::fingerprint(&[f32] at 22.05 kHz) -> Vec<(hash, frame)>`, frames at `fingerprint::FPS` (≈43.07/s) from the start of the analysed audio (the file for imports, the capture for live songs, which is also where `Song.heard.beatTimes` count from). Beat maps: `Song.backing ?? Song.heard`, `beatTimes` per chart beat; chart beat 0 is a 1.

**Library (new `crates/midzer-hear/src/recognise.rs`)**
- `Library`: the stored songs, each `{ id: u32, beats: Vec<f64> /*song s*/, chords: Vec<Option<Chord>> /*per beat*/, seconds: f64 }`, plus one sorted `Vec<u64>` index of `hash << 32 | song << 16 | frame`. Frame as u16 covers 25 min at FPS; looked up with `partition_point`. About 8 bytes a landmark: ~20 k landmarks a 4-min song, so 100 songs ≈ 16 MB.
- `Library::add(id, landmarks, beats, chords, seconds)`, `remove(id)`, `clear()`; the index is rebuilt on change (rare).
- `Library::recognise(&[Landmark]) -> Option<Match { song, offset_frames, votes }>`:
  - every heard landmark votes for (song, stored frame − heard frame), the offsets ±1 frame pooled;
  - a match needs ≥ `MATCH_VOTES` (start at 25 for 8 s heard; tuned on real songs) and ≥ 4× the best vote for another song or an offset more than a beat away.

**Listener (`hearing.rs`)**
- `set_library(Library)`. Without one, nothing changes (so `hear_eval` and the tests stay as they are).
- Recognising: every `RECOGNISE_EVERY_S` (2 s) of sound, fingerprint the last `RECOGNISE_S` (8 s) of `audio` and call `recognise`. It's cheap next to Beat This!, and runs in `step` like the networks.
- A match sets `Follow { song, delta, since }`, where `delta` maps stream samples to song seconds (`song_s = stream_s + delta`). It is exact to about a frame (23 ms) and refined below:
  - Beat This! keeps running at its steady rate while following. Its beats (made sub-frame with the parabola from `offline.rs`, moved to a shared helper) are compared with the stored beats through `delta`; if ≥ 4 are within 70 ms, `delta` moves halfway to their median difference.
- While following, `step` does not fit or merge (the state is kept as is) and says:
  - `Heard::Grid` from the stored map about once a second: the newest stored beat before the stream's end, in engine frames; the period of the stored beats around it; `one` from the beat's chart index mod 4; confidence 1.
  - `Heard::Chords` from the stored chords, from 10 s back to 8 beats ahead, not marked predicted (they are known). BTC doesn't run.
  - `Heard::Follow(Some { song, frame, song_s, votes })` when it starts or moves, `Heard::Follow(None)` when it ends.
- It ends:
  - past the stored song's end (a partial live song heard further): back to listening, quietly;
  - when two recognitions in a row while there is sound match neither the song near the expected place nor anything: the match is lost; or another song matches. Both count as a song change: the capture under way is finished there (`finish()`) and the listener `forget()`s, as after a silence;
  - when the same song matches at a place more than a beat away from where following expects it (a seek): `delta` jumps there, without a change;
  - at a reset or silence, or by `ignore(song)` ("Not this song"), which keeps that song from matching until the next reset or silence.
- `Capture` gains `followed: Option<u32>`: the song followed for the most of it.

**ffi (`ffi.rs`) and plumbing**
- `mzh_alloc(bytes)` for the library data; `mzh_library_clear`, `mzh_library_add(id, landmarks_ptr, len, beats_ptr, n, chords_ptr, seconds)` (chords as the existing code, root · 16 + quality or −1, one i32 per beat), `mzh_library_remove(id)`, `mzh_ignore(id)`.
- `mzh_step` returns 3 for a follow: song id (−1 none), engine frame, song seconds, votes. `mzh_song_followed` → id or −1.
- `hear.worker.ts`: `{type:'library', songs}` (replaces it all, buffers transferred), `{type:'ignore', id}`; replies `{type:'follow', id, frame, seconds}` and `song` gains `followed`.
- `EngineHandle`: `setLiveLibrary(songs: LiveLibrarySong[])`, `ignoreLiveSong(id)`, `onLiveFollow(listener)`; `LiveSongCapture.followed?: number`.
- Desktop: `live_library` takes the songs as one raw body (like `track_load`), into `Shared.library` with a generation the hear thread picks up; `live_ignore`; `UiUpdate::LiveFollow`; `LiveSong` gains `followed`. Type-checked only, as before.

**App**
- `apps/web/src/songs/liveLibrary.ts`: builds `LiveLibrarySong`s for songs with a fingerprint and a beat map (`backing ?? heard`): `beatTimes`, a chord code per beat (resolving `''` to the chord before, through `parseChord` and the protocol's quality order), `seconds`. Ids are small numbers given per session, mapped back to song ids.
- `session.ts`:
  - sends the library when a live source opens and again (debounced) when songs are kept, deleted or analysed again;
  - state `liveFollow: null | { songId, title, bpm }` from `onLiveFollow`; `notThisSong()` → `ignoreLiveSong`;
  - `onLiveSong` with `followed`:
    - the stored song is an import or complete: the capture is dropped, not analysed (it is a song already known);
    - a partial live song, and the capture is complete or ≥ 10 s longer: analysed and offered as "Heard more of <title>" with Update / Dismiss. Update replaces its `heard`, chart and fingerprint, keeping the title, artist and style (the confirm is the button, as with "Analyse again");
    - otherwise dropped.
- `PlayAlong.tsx`: a Stage/club chip "Following <title> · 118 BPM" with "Not this song", while following. The "Songs heard" card gains the update variant.

**Tests**
- recognise: two songs in a library (the fingerprint test's `tune`, moved where both tests share it): an 8 s noisy excerpt of the second matches it within a frame; other music and silence match nothing; removing a song stops it matching.
- hearing, with the bundled networks as in the slice 1 and 3 tests:
  - with a library holding a tune and its beat map, playing it from 40 s in follows it within ~10 s and the grids land on the stored beats (within 10 ms) with the stored 1;
  - jumping 30 s on (a seek) moves the follow; switching to other music ends it and finishes the capture; "Not this song" stops it; past a partial song's end it stops quietly;
  - a capture of a followed song carries `followed`.
- Real songs (`hear_eval` gains `KNOWN=1`): each song is first analysed whole (`hear_song` + `fingerprint`) into a library, then played in from 60 s in, as if a source opened mid-song. Report the time to follow and the beat/1 scores against `ref/`, next to the live-only scores. Expect beats near the offline 0.93 and the 1 near the stored phase once following, within ~5–10 s.
- Vitest: `liveLibrary` (chord codes per beat, songs without a map or fingerprint left out); the hear message decode; the capture routing in the session (known → dropped, partial → update offer).
- Playwright (rebuild first): with the fake source, learn the song as in slice 3 (Save this song → Keep), then Start over: the chip shows "Following Heard …".

**Open, by design**
- A change between two unknown songs without a silence (gapless playlists) is still captured as one; novelty detection (tempo, key, chroma) is left for later.
- The bar phase is one per song, as in slice 2; wrong 1s are fixed in slice 5.

## Slice 5: bars of any length, then fix beats and chords by hand
The user chose (2026-10-24):
- **bars of any length** (1–7 beats) in the chart, the engine, the tempo map and following, rather than squeezing odd bars into 4/4;
- the **analysis places the odd bars itself** from the network's downbeats, in this slice;
- **"Analyse again" asks** when a song has hand fixes: "Keep my fixes" or "Replace all".

Why: wrong 1s come after odd bars (2, 3 or 5 beats), which one bar phase per song can't hold (downbeats 0.63 F against the network's own 0.91). A 4-beat chart can't hold them either, so an editor alone couldn't fix them.

It is built in two parts, measured in between: **5a** bars of any length + bars from the network's downbeats; **5b** the timeline editor + keeping fixes.

The rule throughout: **song beats stay numbered from the start of the song, one after another**. So `SongChord.beat`, `TrackBeat.index`, `SongPosition.beat`, `SongWait.tick`, `Song.backing/heard.beatTimes` and the library's per-beat chords keep their meaning. Only what turns bars into beats and back changes: `bar * 4` → `bar_start(bar)`, `beat / 4`, `beat % 4` → a lookup. Jam, the drill progressions and live sources with no stored song stay 4/4.

### 5a. Bars of any length

**Chart model (`apps/web/src/songs/song.ts`)**
- A bar's length is `bar.beats.length`, 1–7. `BEATS_PER_BAR` stays as the default. New helpers: `barStarts(bars)` (prefix sums, with the total at the end), `totalBeats(bars)`, and `barAt(starts, beat) -> { bar, beat }`.
- Notation: a `[3/4]` tag (any `[n/4]`, n 1–7) sets the length from its bar on, until the next one, as a time signature does. `%` copies the previous bar, its length included.
- `spread(tokens, n)`: one chord fills the bar; two split it at n/2 when n is even; otherwise one per beat from the start (as today for 3 chords in 4/4).
- `formatBar` is the inverse; `formatChart` writes `[n/4]` where the length changes. It round-trips with `parseChart`.
- `emptyBar(n = 4)`. `chartCommands` sends chords at the bar's start beat and a `songBar` for each bar that isn't 4 beats long. `MAX_TRACK_BEATS` = `MAX_BARS * 7 + 1`, matched in Rust.
- `library.ts` sanitise accepts lengths 1–7. Stored songs are all 4/4 and stay valid: no DB version.

**Engine (`crates/midzer-core`)**
- `command.rs`: a new `Command::SongBar { bar, beats }`, sent after `SongClear` (which resets every bar to 4). Encoded with the song ops, plus the round-trip test.
- `song.rs` `SongPlayer`:
  - adds `bar_beats: [u8; MAX_SONG_BARS]` and `bar_start: [u16; MAX_SONG_BARS + 1]`, recomputed on `SongBar` (that is a command, not the audio path). `slots` is sized `MAX_SONG_BARS * 7`.
  - `bar_start(bar)`, `bar_of(beat)` (a binary search in `bar_start`; no allocation).
  - `on_beat()` no longer takes `beat_in_bar` from the band. It returns it, with the bar's length, from its own `next_beat`. Seeks, loops, `upcoming`, `slot`, `chord_at` and `start` use `bar_start`.
  - Marks are asked for on beat 1 as now, or beat 0 in a 1-beat bar.
- `band.rs`: while a song plays, the bar comes from the song. The band keeps `bar_start_tick`, the bar's length and a pattern-bar counter, set on each downbeat:
  - `schedule_range` and `restrike` schedule only the current bar, `bar_start_tick .. + beats·PPQ`. The walk is beat by beat, so a bar's length is known before it is scheduled.
  - `Pattern::bar(index, beats)` (`style.rs`): a shorter bar plays the pattern's first `beats` beats. A longer one plays its 4 beats and then repeats its beats 3–4 (the second half) for the rest. Patterns stay written in 4/4.
  - The `Beat` event, the click accent, `waits_on_beat` and the conductor get the song's bar and beat-in-bar.
  - Without a song, nothing changes (4 per bar).
- `conductor.rs`: the half-bar decision is at `beats / 2` (beat 0 in a 1-beat bar).
- `drummer.rs`: a fill takes the last two beats of the bar (the whole bar if it is shorter). The count-in stays 4 hits.
- `backing.rs`:
  - starting a bar early, `jump_target`, `chart_beats`, seeks, loops and the `Beat` event go through `chart.bar_start` / `bar_of`. `play()` gets the chart.
  - The bpm per bar is measured over that bar's beats.
  - `judge()` maps the beat through the chart: `sixteenth = (k − bar_start) · 4 + nearest`, which goes up to 27 (it still fits the u8). `keys_strike` uses the absolute sixteenth.
- `event.rs`: only docs change (`Beat.beat` up to 6, `Judged.sixteenth` up to 27).

**Following a stored song with odd bars**
- `live.rs` counts its bars mod 4 from the guide's 1. With odd bars that is wrong for up to a guide's interval after each one.
- New `Command::LiveBar { frame, beats }`: "a bar of `beats` beats starts at the beat at `frame`".
  - The listener sends each bar line from the stored map a few beats ahead, since it knows them.
  - `Live` keeps up to 8 upcoming bar lines in a fixed ring. Its `Beat` events, its 1 and the judging use them, and fall back to mod 4 once they run out (the song ended, or a live source with no stored song).
  - `LiveForgot`/`LiveReset` clear them.
- `hearing.rs` follow mode: the stored 1s come from the song's bar starts, not `j % 4`. The vote for the 1 (stored 1s 0.3 each, the network's downbeats fully) becomes a vote for **which stored beat is beat 0 of the chart**, i.e. how the stored bars line up. The bars then come from the map.
- `recognise.rs` `StoredSong` gains `bar_starts`. ffi `mzh_library_add` gains a `bar_beats` pointer and count; `hear.worker.ts`, `liveLibrary.ts`, native `decode_library` and the desktop `live_library` body gain the same. `Heard::Bars` / step result 4 carries upcoming bar lines.

**Analysis: bars from the network's downbeats (`crates/midzer-analyze`)**
- `bar_phase` → `bar_lines(beats, downbeats) -> Vec<usize>` (the beat each bar starts on):
  - every network downbeat snaps to its nearest beat;
  - a gap of 4 is a bar. Gaps of 1–3 or 5–7 are kept only when the next two gaps are 4 again (or the song ends), so a stray or missed downbeat doesn't make two odd bars in a row. Pieces that add up to 4 (2 + 2, 1 + 3) merge into one bar; gaps of 8 or more split into 4s from the first downbeat;
  - before the first downbeat is a pickup, left out as now;
  - a song mostly in 3 (the waltz check) gets bars of 3, and the "midzer counts four" warning goes.
  - The thresholds are tuned on `files24`.
- `Analysis` gains `bar_beats: Vec<u8>`; `beat_times` and `chords` stay one per beat, now `sum(bar_beats)` of them; `bar_times` are the bar starts.
- `chords.rs` `Meter::Bars { phase }` → the place of each beat in its bar (`Vec<u8>`), with the same change costs at 0 and the half bar.
- `structure.rs` sections: each bar's chroma resampled to 4 slots, so the 48-wide feature stays. The DSP fallback stays 4/4 with one phase.
- `offline_dump.rs`, `analyze.rs`, `live_eval.rs`, `hear_eval.rs` take the 1s from `bar_beats`.

**Everything else that walks a chart**
- `audio.ts` `songFromAnalysis` groups the beats by `barBeats`. Transcription MIDI and `smf.ts` write a time-signature event where the length changes.
- `midiImport.ts` reads the file's meters into bar lengths (n/4 directly; n/8 as n/2 beats when n is even, else warned as now).
- `chordpro.ts`: `{time: n/4}` sets the length.
- `session.ts`: `songPosition` and the store's bar from `barAt`; `editSong` resends `songBar`s.
- Learn: `practice.ts` (loop range, song end), `targets.ts` (chord asked again on each bar line), `lanes.ts` `barLines(starts)`. `melody.ts` checks bars against the chart's lengths.
- UI: `ChartView` bar progress and beat slots per bar (CSS for 1–7 slots), `NowPlaying` beat dots, the "one to four chords" help texts, "Add bar" with the selected bar's length. `drums/judging.ts` names spots by beat (`sixteenth % 4`) and treats "the backbeat" as beats 2 and 4 of 4-beat bars only.
- `protocol.ts`: `songBar`, `liveBar`, the docs on `beat`/`judged`.

**Tests (5a)**
- core: a song of 4, 2, 4, 3, 4 beats plays its chords on the right beats, sends `Beat` with the right bar/beat, loops and seeks on its bar lines, its patterns cut and extend as above; a recording's tempo map against the same chart (judged bar/sixteenth); `SongBar`/`LiveBar` round-trips; `Live` with bar lines ahead counts the odd bar right and falls back to 4 after.
- analyze: `bar_lines` on downbeats with an odd bar, a stray downbeat, a missed one, 2 + 2, a waltz; `analyze_mix` on a band signal with a 2-beat bar.
- hear: following a tune with an odd bar sends its bar lines; the vote lines up the stored bars.
- Vitest: notation round-trips with `[3/4]` and `%`; `barStarts`/`barAt`; sanitising; `songFromAnalysis` with `barBeats`; MIDI import of 3/4 (the "read as 4/4" test changes); ChordPro `{time}`; targets and lanes on odd bars; `liveLibrary` sends the bar lengths.
- Playwright (rebuild first): a chart with `[2/4]` plays through with the bar count right.
- **Measure**: `offline_dump` on `files24` → `score.py`. Expect downbeats from 0.63 towards the network's 0.91, beats and chords unchanged (0.93, 0.89 majmin). Then `KNOWN=1 START=60 hear_eval` for the 1 while following (87 % now).

### 5b. The timeline editor, and keeping fixes

**Drawing**
- A timeline in `SongScreen`, under the chart, shown in edit mode for songs with a beat map (`backing ?? heard`). It is one canvas, scrolled and zoomed (pinch / ctrl-wheel), in the Stage/club style. From top to bottom:
  - the level: `Song.heard.envelope` for live songs. Imports store it too, as `SongBacking.envelope` (it already comes out of every analysis). Older imports get it by decoding their stored recording in the browser (`decodeAudioData`, RMS per 10 ms) when the editor first opens, and save it;
  - beat lines, with bar lines (and their numbers) brighter;
  - chord blocks, one per chord held, on the beats.
- While the recording plays, a playhead. Clicking the timeline outside a tool seeks to that bar.

**Tools** (they act on the selected beat or beats; shift-click selects a range)
- **The 1 is here**: the beat becomes a bar line. The bar it was in ends there, as a shorter bar (1–3 beats; if that is a 1-beat bar, the player is asked whether it is right). The bars after it are laid again in their length from the new line, up to the next bar line fixed by hand, or the end. Sections, fills and lyrics stay on their beat.
- **Not a 1**: the bar line goes; the bar joins the one before (up to 7 beats).
- **Nudge** (←/→, 10 ms; with shift, 1 ms) or **drag** a beat line; a selection moves together. Dragging snaps to the nearest onset in the level when close.
- **Even out**: the selected beats are spaced evenly between the first and the last. This replaces the outline's "tempo change point": a change is evened out on each side of it.
- **Half / double tempo** (selection, or the whole song): every other beat goes, or one goes between each pair. Bar lines are laid again in 4s from the first bar line in it; each chord stays where it was in time.
- **Tap along** (songs with a recording): play from a bar and tap (space, or a pad/MIDI note) through the selection. The taps, moved by the player's sync (`tapOffsetMs`), become its beats and are evened out where taps are missing. Live-learned songs have no audio to tap with, so the tool is hidden for them.
- **Chords**: drag a block's edge to another beat; click a block to type a chord (`parseChord`) or pick one from the key's chords. It edits `bar.beats` in place, so the chart grid and the timeline are one edit.
- **Undo / redo** across both the chart grid and the timeline (an edit history in `session.ts`, 100 steps, cleared on closing the song).
- Every edit goes through `editSong` (the chart and `songBar`s resent, the tempo map resent while the recording plays) and is saved with Save, as chart edits are now.

**What the song remembers (`song.ts`)**
- `Song.fixes?: { beats?: true; bars?: number[] /*bar lines set by hand, as song beats*/; chords?: number[] /*beats whose chord was set by hand*/ }`, filled in by the tools and the chart grid. Sanitised in `library.ts`.

**Analyse again with fixes** (`AnalyseAgain` in `SongScreen`, `analyzeSongAgain` in `session.ts`)
- Without `fixes`: as now.
- With them, the confirm offers **Keep my fixes** or **Replace all**. Keep merges:
  - if the beats were fixed, the old beat map and bars stay. The new chords move onto the old beats by time: each old beat takes the chord over its middle;
  - otherwise the new beat map comes in, with bars laid from the new analysis but the bar lines fixed by hand kept (moved to the new beat nearest them in time);
  - chords set by hand stay, moved by time onto the beats if those changed;
  - sections, mix melody and the fingerprint are replaced as now.
- **Replace all** drops `fixes`.

**The listener gets the fixes**
- The library is sent again (debounced, as when a song is kept) when an edited song is saved, so the next look-up follows the fixed map. Until then, the old one.
- The outline's "corrections tune the fingerprint offset" isn't needed: a live song's beats and its fingerprint both count from the start of the capture, so moving beats moves them against the audio, which is what is wanted.

**Tests (5b)**
- Vitest, for each tool as a pure function on (`bars`, `beatTimes`, `fixes`) in a new `songs/timeline.ts`: the 1 is here (a short bar, bars laid again up to a fixed line); not a 1; nudge; even out; half/double keep chords in time; tap with a missing tap; chord edge and chord type; undo/redo; merging on "Keep my fixes" (beats fixed / not fixed, hand chords kept).
- Playwright (rebuild first): import the test tune, open the editor, "The 1 is here" on a beat → the chart has a short bar and plays through; Save; "Analyse again" asks and Keep keeps it.
- Desktop parts are type-checked only, as before.

**Open, by design**
- Meters with eighth-note beats (6/8 felt in 2) are counted in beats as heard: a 6/8 song is 2 beats a bar if the network hears 2.
- Band patterns stay written in 4/4 and are cut or extended in odd bars. Patterns per meter (a real 3/4 waltz, 5/4) are later.
- Live sources with no stored song stay 4/4.
