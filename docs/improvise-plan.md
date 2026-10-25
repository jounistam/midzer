# Improvise: solo mode for Play along

## Context
Play along currently teaches chords in 5 levels (`apps/web/src/playalong/levels.ts`). The next step is **improvising**: anyone should be able to play a groovy solo over any song from the first minute, and do more of it themselves as they move up the levels. On level 1 the player taps one lit key, the song's home note, and midzer plays a solo that fits the song. **Tapping faster raises the intensity** and **velocity sets the loudness**, so the player shapes the feeling. Higher levels hand over first the shape of the solo, then the notes, then the rhythm.

The user's choices (2026-10-25):
- **Level 1 key:** the song's home note (tonic). It stays lit and rarely moves.
- **Scope:** Play along only. The engine part is reusable, so Jam and Songs can use it later.
- **Extras in this round:** call and response, hold for a long note, replay my solo, and a drum solo.

## Architecture
The solo is generated **in the engine** (`crates/midzer-core/src/solo.rs`), so its notes land sample-accurately on the beat grid the player hears. The UI only works out the key and chord and sends them to the engine.

- **Beat grid:** add `Live::grid() -> Option<Grid>`, which returns the next heard beat's frame, its index, the period and the beat in the bar. It reuses the `heard(j)` maths and `place()` from `Live::show_beats` (`live.rs:600`), including the offset, and returns `None` until the tracker locks.
- **Harmony:** add a new command `SoloHarmony { tonic, scale: u16, chord_root, chord: u16 }`, built from pitch-class bit sets. The UI sends it from its 50 ms tick in `followLiveChords` (`session.ts:1451`), but only when it changes. The chord switches `SWITCH_MS` early, as `keyHints` does.
- **Key detection (TS):** add `playalong/scale.ts` with `keyOf(spans | followed song chords)`, which scores the 24 keys from the chord pitch classes weighted by duration.
  - Hysteresis: the key changes only when another key wins clearly for 8 s or more, so the lit key stays put.
  - When a song is followed, its stored chords decide the key (`chordsUsed`, `session.ts:1347`).
  - Scale: major pentatonic (0 2 4 7 9) for a major key, minor pentatonic (0 3 5 7 10) for a minor key. The blue note (6) is used only as a passing note at high intensity.
- **Mode command:** `SetSolo { level: u8 (0 = off), call: bool }`.
- **Note routing:** in `Engine::handle` NoteOn/NoteOff (`engine.rs:94-111`), when solo is on and live is active, notes go to `Solo` instead of sounding directly. Drum notes go the same way from `drum_note`. `Solo` keeps a fixed key→sounding-key map so each NoteOff matches the note that actually sounded.
- **New events:**
  - `SoloFeel { energy: u8 }`, at most 10 per second, for the intensity meter.
  - `SoloNote { key, velocity (0 = off), bar: u16, tick: u16 (PPQ into the bar), own: bool }`, for lighting keys and recording the solo.
  - `SoloCall { phase, hit, of }`, for call and response.
- **Encoding:** mirror the new commands and events in `command.rs` (op codes and round-trip tests) and in `packages/audio-web/src/protocol.ts` (with `protocol.test.ts`).
- **No allocation:** `Solo` uses only fixed arrays, a xorshift RNG, and a fixed list of pending note-offs.

### The generator (`solo.rs`)
- **Energy:**
  - The tap rate is measured as an EMA of taps per beat over about the last 2 beats.
  - Energy rises with the rate: about 0.5 taps per beat is calm, 1 is eighths, 2 is sixteenths, and 3 or more is runs, triplets and a higher register.
  - Energy decays over about 1 beat after the last tap. At 0 the phrase ends on a chord tone and the solo rests, so it "breathes".
- **Rhythm:**
  - Every 16th slot on the grid gets a play probability, weighted by groove: the beats, the "and"s, and syncopated anticipations.
  - The weights depend on the energy.
  - Short 2–4 note rhythm cells repeat once, because repetition is what makes it sound like a groove.
- **Notes:**
  - Notes move along a ladder of the scale, mostly by steps, sometimes by a leap of 2–3, and turn around at the edges of the range.
  - On strong slots they prefer chord tones of the current chord.
  - Phrases end on a chord tone.
  - The range and centre rise with energy. At high energy there are grace notes (a quick approach note from a semitone below) and repeated-note bursts.
- **Response:** a tap after silence sounds a pickup note **at once**, so the tap clearly causes the sound. The notes after it fall on the grid.
- **Hold for a long note:** while a driving key is held, the solo lands on a chord tone at the next strong slot and sustains it. If the synth supports CC1, it adds vibrato after half a beat. This needs a new `EngineEvent::Control`; check during the build and leave vibrato out if the synth has no CC1. Releasing the key lets the solo carry on.
- **Deterministic:** the RNG seed is settable, so the tests are repeatable.

### Levels (keys)
| # | Name | Player does | midzer does |
|---|------|-------------|-------------|
| 1 | One key | Taps the lit home note. Speed sets the intensity, velocity the loudness, and holding gives a long note. | Plays all the notes and the rhythm. Other keys are silent. |
| 2 | High and low | Taps one of 3 lit keys (home, fifth, home an octave up), which steer the register and direction. | Plays the notes and the rhythm. |
| 3 | Your notes, our groove | Plays the lit scale keys (chord tones brighter). A note outside the scale snaps to the nearest scale note. | Adds groove fills on the grid around the player's notes, as dense as the intensity. |
| 4 | Safety net | Plays every note and the rhythm. | Only snaps notes outside the scale. The scale stays lit. |
| 5 | On your own | Plays freely. | Nothing. The feedback shows each note as a chord tone, a scale note or outside, plus the timing. |

At level 1 the space bar also plays the lit key, and the lit key shows its QWERTY letter.

### Drum solo (`instrument === 'drums'`)
| # | Player does | midzer does (on `PLAYER_DRUM_CHANNEL`) |
|---|-------------|------|
| 1 | Taps the lit snare pad. | Plays the groove; faster tapping makes it busier, adds tom fills and a crash on the next 1. |
| 2 | Kick, snare and tom pads choose a kick-led groove, the backbeat, or a fill. | Plays all of it. |
| 3 | Plays the kick and snare. | Plays the hi-hat, ghost notes and fills at bar ends when intensity is high. |
| 4 | Plays everything. | Nothing; the existing judging gives the feedback. |

### Call and response (a toggle within Solo, at any level)
- midzer plays a call of 1 bar (later 2 bars), shown as "Listen". The player answers in the next bar, shown as "Your turn".
- **Levels 1–2:** each tap plays the call's next note, so tapping the call's rhythm brings the lick back. The answer is scored on rhythm, with each onset counting if within ±1/16 of the call's.
- **Levels 3 and up:** the player plays the notes. The next key to play is lit, and the answer is scored on notes and rhythm.
- The calls come from the same generator at moderate energy: they start at 2–5 notes and get longer as the player succeeds.
- On drums the calls are short rhythms on the kit.
- Results come through `SoloCall` and show as, for example, "4 of 5".

### Replay my solo
- **Recording:** the UI records `SoloNote` events during a run, relative to the run's first bar. Each note is marked as the player's own or midzer's.
- **Playback commands:** `ReplayClear`, `ReplayNote { tick, len, key, velocity }` and `ReplayPlay { on }`.
- **Playback:**
  - `Solo` owns a fixed replay buffer of about 1024 notes.
  - If the beat is locked, playback starts on the next bar's 1 of the live grid.
  - Otherwise it plays on its own clock at the run's tempo, with a soft click.
- **Display:** after a run, a "Your solo" card (`components/SoloRoll.tsx`, a small canvas) shows the notes bar by bar. The player's own notes and midzer's notes have different colours, with "You played N%". The card has buttons to play and stop the replay.

## UI changes
- **`apps/web/src/playalong/solo.ts`:**
  - `SOLO_LEVELS` and `DRUM_SOLO_LEVELS` (name and detail), plus `parseSoloLevel`.
  - `soloKeys(level, harmony)` returns the keys to light. Add new hint values for the home key, scale keys and chord tones in `live.hint`, with CSS in `index.css`, and check how `Keyboard.tsx` draws hints.
- **`session.ts`:**
  - New prefs: `alongMode: 'chords' | 'solo'`, `soloLevel`, `soloCall`, saved like `alongLevel` (`session.ts:236/351/818`).
  - Setters that send `SetSolo`.
  - Harmony and lit keys in `followLiveChords`.
  - Handlers for `SoloFeel` (as `soloFeel` state), `SoloCall` and `SoloNote` (records the replay).
- **`screens/PlayAlong.tsx`:**
  - A "Chords | Solo" switch in the panel, for both instruments; for drums, Chords is the current behaviour.
  - In Solo, a `SoloLevelPicker` and a "Call and response" toggle.
  - The stage gets a `SoloStage`: a large "Feel" meter (calm → fire), the call-and-response status, and the keyboard or kit with the lit keys.
  - The `HitFeedback` and `TimingReport` sections are hidden at levels 1–3 (taps aren't judged there) and shown at levels 4–5.
  - After a run, the "Your solo" card.
- **Live judging:** in `live.rs`, pass the strike or hit to `Live::queue` only when solo is off or the solo level is ≥ 4 (keys) or 4 (drums).

## Slices (each one builds, passes its tests, and is committed by the user)
1. **Engine core and keys level 1.**
   - Engine: `solo.rs` (energy, rhythm, notes, pickup), `Live::grid`, and the `SetSolo`, `SoloHarmony`, `SoloFeel` and `SoloNote` protocol.
   - UI: `scale.ts`, the Chords/Solo switch, the Feel meter, and the lit home key.
2. **Keys levels 2–5 and hold for a long note:** snapping, groove fills, and the feedback on note fit.
3. **Call and response:** for keys at all levels.
4. **Replay my solo:** the replay commands, `SoloRoll`, and the card.
5. **Drum solo:** levels 1–4, plus call and response on the kit.
6. **Docs:** update the Status section of `docs/PLAN.md` and the architecture memory.

## Verification
- **Rust** (`cargo test -p midzer-core`; never run `cargo fmt` on whole files). Tests in `solo.rs` use a fixed seed and a synthetic grid:
  - With no taps, there are no notes.
  - More taps per beat give more notes per bar, monotonically, and a higher mean pitch.
  - Every note is in the scale at levels 1–4.
  - At levels 1–2, every note after the pickup is on a 16th of the grid.
  - The solo goes silent within 2 beats after the taps stop.
  - Holding gives one long note.
  - Snapping at level 4 works, and level 5 passes notes through unchanged.
  - Call-and-response scoring works.
  - Replay timing works.
  - Nothing allocates: no `Vec` is created in `process`.
  - Also: round-trip tests in `command.rs`, `engine.rs` routing tests (with solo on, the player's keys don't sound directly), and a `Live::grid` test against the `Beat` events.
- **TS:** `pnpm -C apps/web test`, with `scale.test.ts` (key from chords, hysteresis, followed song), `solo.test.ts` (lit keys per level) and the `protocol.test.ts` encodings.
- **e2e:** rebuild the wasm and the web app first. Add a solo test to `apps/web/e2e/along.spec.ts`: switch to Solo, tap the lit key, see the Feel meter rise and keys light. Existing along failures (sync, spread, learn result) are known and not regressions.
- **By ear:** play along with the user's mp3s in a browser tab, at keys levels 1–3 and drums level 1. Check that slow taps sound calm and that fast taps build up without clashing with the chords. The generator's weights get tuned here.

## Handoff notes (read before starting the next slice)

### Slice 1: done, committed as c1850b0 "chore: impro slice 1"
**Engine (`crates/midzer-core/src/solo.rs`, `Solo`)**
- `Grid { at, index, period, one }`: the frame where the player hears beat `index`, frames per beat, and a beat index that is a 1. It comes from `Live::grid()` (`live.rs`, next to `show_beats`), which adds the offset and returns `None` until the tracker locks. Sixteenths are counted as `q` (four per beat). Helpers: `frame(q)`, `sixteenth_at(frame)`, `beat_in_bar(q)`.
- API:
  - `set_level(level, lead_ms, out)`: a level change silences the solo.
  - `set_harmony(tonic, scale_bits, chord_bits)`: the scale falls back to the major pentatonic of the tonic when 0.
  - `press(key, vel, now, grid, out) -> bool` (consumed), `release(key, now, out) -> bool`, `silence(at, out)`, `process(frames, now, grid, out)`, `home()` = 60 + tonic.
- Level 1 logic (the level number is otherwise ignored for now, so every level acts like level 1):
  - Only `key % 12 == tonic` drives the solo; other keys are swallowed silently.
  - Each tap updates `rate`, an EMA of taps per beat capped at 4. The first tap, or a tap after `gap()` = 1.25/rate + 0.5 beats, resets the rate to 1. `touch` is an EMA of the velocity.
  - `energy` follows `energy_of(rate)` = 0.25 + 0.25·log2(rate/0.5), rising over 0.25 beats and falling over 0.75 beats. The target is 0 once the taps stop driving it, as checked by `driven()`.
  - Pickup: a tap more than 1 beat after the last note plays a note at once (frame 0), and the grid slot within 1/8 beat after it is skipped. This skip fixed a bug where a note-off was sorted before its note-on on the same frame.
  - Per beat, `new_beat()` decides the 4-bit sixteenth `mask`:
    - phrases last `2 + rand(3) + energy*4` beats;
    - the last beat of a phrase plays one long ending note (2 beats), followed by `rest_left = round((1-energy)*2)` rest beats;
    - with 40 % chance the beat repeats the last beat's mask;
    - otherwise `density(energy, pos, beat_in_bar)` uses thresholds pos0 0.1, pos2 0.35, pos3 0.55, pos1 0.65, plus a little extra on beats 1 and 3;
    - once the taps stop driving it, one ending note follows, then silence.
  - Notes (`pick(strong)`):
    - the ladder is the scale keys from `home-3+5e` to `home+10+9e` (fixed `[u8; 40]`);
    - steps are 0/1/2/3 with chances 10/60/20/10 %, the direction is kept with 70 % chance, and the walk bounces at the edges;
    - on strong slots (beats 1 and 3, endings, pickups and long notes) it snaps to the nearest chord tone within 2 steps;
    - `chord_tones()` falls back to the home triad taken from the scale.
  - When energy is above 0.8, a slide comes in a 32nd early from the scale note below (`grace`).
  - The solo is monophonic: `sounding` and `grace` are each `Option<(key, off_frame)>`, `end_note` ends the previous note, and `release_due` ends notes in later blocks.
  - Velocity: `0.6·touch + 30 + 20·energy` + accent (+8 on pos0, −10 on pos1/3) ± 5.
  - Hold: holding the key for 0.3 beats sets `Long::Waiting`, which plays a chord tone on the next even sixteenth with an infinite length (`Long::Sounding`). Energy freezes meanwhile. Releasing ends the note and sets `last_tap` to now.
  - `SoloFeel { energy: u8 }` is sent at most every 0.1 s, and only when it changes.
- Engine (`engine.rs`):
  - `solo = !drums && live && solo.level() > 0`.
  - NoteOn goes to `solo.press`, and a consumed tap with a grid calls `live.play` to start a run, so taps are never queued for judging. NoteOff goes to `solo.release`.
  - `solo.silence` runs on AllNotesOff, Stop and LiveReset.
  - `solo.process` runs right after `live.process` in the live branch of `process`.
- Protocol:
  - `Command::SetSolo { level ≤ 5, call, lead_ms 0..=500 }` is op 53: a = level | call<<8, b = lead f32 bits.
  - `Command::SoloHarmony { tonic < 12, scale, chord ≤ 0xfff }` is op 54.
  - `EngineEvent::SoloFeel` is op 18. It is mirrored in `packages/audio-web/src/protocol.ts` (`setSolo`, `soloHarmony`, `soloFeel`) and ignored in `midzer-synth`.
- Tests:
  - `solo.rs` tests use the helper `run_holds(solo, beats, &[(tap_frame, hold_frames)])`, which returns the notes as `(start, end, key, vel)`. The grid is 120 BPM at 48 kHz (PERIOD 24 000), with blocks of 128 frames.
  - Other tests: `engine.rs` `a_solo_takes_the_keys_over_live_audio` and `live.rs` `the_grid_is_where_the_beats_are_shown`.

**UI**
- `apps/web/src/playalong/scale.ts`:
  - `keyScores` (Krumhansl profiles, root weighted ×2, plus 0.15 × the share of chords on the home note of the same mode), `keyOf`, `keyName`, `pentatonic` (bits), `chordBits`;
  - `KeyFollower` keeps the key: a rival needs a margin of 0.1 held for 8 s. It also has `set` and `reset`.
- `apps/web/src/playalong/solo.ts`: `AlongMode`, `SoloLevel`, and `SOLO_LEVELS`, which lists level 1 only, so add levels there. Also `parseSoloLevel`, `parseAlongMode`, `homeKey`, `TAP_HINT` = 4, `soloHints(level, key)` and `feelWord`.
- `session.ts`:
  - Prefs: `alongMode`, `soloLevel`, `soloCall`. State: `soloFeel`, `soloKey`.
  - Setters `setAlongMode`, `setSoloLevel` and `setSoloCall` call `sendSolo()`, which sends level 0 unless the mode is solo on keys, with `leadMs = outputLatencyMs`. `sendSolo` is also called at engine start and on `setInstrument`.
  - `sendSoloHarmony(spans, ms)` runs inside the 50 ms `followLiveChords` tick:
    - it weights the heard (not predicted) spans of the last 30 s (`KEY_WINDOW_MS`);
    - a followed song's chart sets the key through `onLiveFollow` → `chartChords` → `liveKey.set`;
    - the chord is the one at ms + 150 (`SOLO_SWITCH_MS`);
    - it sends only when the harmony changes (`soloHarmonySent`).
  - `forgetLiveSong` resets the key.
  - When soloing, the keys lit are `soloHints`; otherwise they are the chord `keyHints`.
  - `spaceTap()`: the space bar plays the home note on solo level 1 on the along screen; otherwise it toggles play.
- `screens/PlayAlong.tsx`:
  - `ModePicker` (Chords/Solo, keys only), `SoloLevelPicker`, and `SoloPlay` (the prompt `solo-prompt`, a large Feel word `solo-feel`, the meter `role=meter name=Feel`, and a Keyboard).
  - `AlongTiming` returns null while soloing at levels ≤ 3.
  - CSS lives in `index.css` under `.solo-feel*`.
- `Keyboard.tsx` draws hint 4 like the other hints (the melody colour). There are no styles yet for scale keys or chord tones.
- e2e: `along.spec.ts` "play along: a solo driven by tapping one key" uses the space bar taps from the fake groove input (C then G, so the key found is C major).

**Open after slice 1:** not tried by ear, and every weight is a guess. The blue note isn't used yet. `call` is passed through but ignored, and there is no vibrato.

### Notes for slice 2 (keys levels 2–5, hold vibrato)
- Make `Solo` act on `self.level`:
  - level 2: three driving keys (home, fifth, octave) bias the direction or centre;
  - level 3: the player's own notes snap to the scale and sound as played, with grid fills by energy;
  - level 4: snap only;
  - level 5: pass through unchanged.
  - Snapped notes need a key→sounding map so each NoteOff ends the note that actually sounded.
- From level 4 on, strikes should be judged again: call `strike_note` and `live.strike` as the normal keys path does. `AlongTiming` already shows the timing at levels ≥ 4.
- Vibrato needs a new `EngineEvent` (a CC on a channel), handled in `midzer-synth` `render`. Check whether its synth (`midzer-synth/src/lib.rs`, SoundFont-based) has a CC call.
- New hint values for the scale and chord tones, with drawing in `Keyboard.tsx`, and more `SOLO_LEVELS` entries.
