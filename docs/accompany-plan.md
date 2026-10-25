# Accompany: patterns for beginners, and chords kept in view while soloing

## Context
Play along on keys has two modes: **Chords** (5 levels in `apps/web/src/playalong/levels.ts`, from one key up to full chords with both hands, with falling chord lanes) and **Solo** (`docs/improvise-plan.md`). Two problems:
- In Solo, the chord lanes and the chord level go away. The player loses sight of the song's chords.
- A beginner who can play the chords at level 1–3 only plays block chords or single roots. They have no way to sound like a real keys player (arpeggios, broken chords, a bass line) without playing every key.

The user's choices (2026-10-25):
- **Solo keeps the chord view:** the chord lanes at the player's chord level (easy or hard, levels 1–5) show in Solo too, as a guide. Every key still drives the solo, and the chords aren't judged.
- **A new mode, Accompany:** `Play: Chords | Accompany | Solo` (keys only). It has levels like Solo:
  1. Tap the chord's root, and midzer plays the whole pattern (bass and broken chord) on the beat.
  2. Hold the root, and the pattern runs while it is held.
  3. Hold the chord with the right hand, and midzer adds the left-hand bass pattern.
  4. Play the pattern yourself, guided and judged.
- **Pattern choice:** a pattern picker (Arpeggio, Broken chord, Pop stabs, Ballad, Bounce). Tapping faster makes it busier within the pattern.
- **Left hand in Solo:** an optional "Accompany with the left hand" toggle in Solo splits the keyboard. Keys below the split drive Accompany (at its level and pattern), and keys above drive the solo.

## Architecture
Accompany runs **in the engine** (`crates/midzer-core/src/accomp.rs`, `Accomp`), next to `Solo`, on the same beat grid (`Live::grid()`), so the patterns land sample-accurately. The UI sends the chord and the settings only.

- **Shared tap feel:** move Solo's tap handling into `crates/midzer-core/src/taps.rs` (`Taps`: `rate` EMA of taps per beat, `touch` EMA of the velocity, `last_tap`, `gap()`, and the energy follower) so `Solo` and `Accomp` share it. The move must not change Solo's behaviour: all `solo.rs` tests keep passing unchanged.
- **Patterns:** author them with the existing step-grid parsers in `style.rs` (`bass("R---5---")` and `chords("1-2-3-")`, PPQ 480, built once in a `OnceLock`, so nothing is allocated on the audio path). Each pattern has 3 variations, A (calm), B and C (busy), and each variation is a bass line plus a chord line. They are starting points, to be tuned by ear:

  | Pattern | A | B | C |
  |---|---|---|---|
  | Arpeggio | bass `R-------R-------`, chord `1---2---3---2---` | chord `1-2-3-4-3-2-1-2-` | chord `1234321212343212` |
  | Broken chord (Alberti) | bass `R-------5-------`, chord `1-3-2-3-1-3-2-3-` | bass `R---5---R---5---` | chord `1323132313231323` |
  | Pop stabs | bass `R-------R-------`, chord `x-------x-------` | bass `R-----R-5---R---`, chord `x--x--x---x-x---` | bass `R-.RR-.R5-.58-5-`, chord `X-ox-ox-x-ox-oxo` |
  | Ballad | bass `R---------------`, chord `--1-2-3-4-3-2---` | bass `R-------5-------` | chord `--1234-3-2341-3-` |
  | Bounce (stride) | bass `R-------5-------`, chord `----x-------x---` | bass `R---3---5---3---` | bass `R-5-8-5-R-5-8-5-`, chord `--x-x-x---x-x-x-` |

- **Voicing:** use `voicing::voice_near` with the chord from `chord.rs`. The bass sits in 36–47 (C2–B2, like `LEFT_C` in `levels.ts`). The chord voicing sits around a centre of 60 (range 52–72) alone, or 52 (range 45–59) when it shares the keyboard with Solo. It is re-voiced near the previous centre at each chord change. Notes still sounding when the chord changes end at the change.
- **Energy → variation:** the tap rate maps to energy (about 0.25 taps per beat → 0.15, 1 → 0.5, 2 or more → 0.85). Below 0.4 plays A, below 0.75 B, and otherwise C, with a small hysteresis. The variation switches only on a bar's 1. Unlike Solo, the energy **holds** between taps while the pattern is driven, so one tap per bar keeps a calm pattern going.
- **Driven:**
  - Level 1: the pattern runs for `KEEP_BEATS` = 8 beats (2 bars) after the last tap on the lit root.
  - Level 2: the pattern runs while the lit root is held. The energy comes from the re-press rate, and the press velocity sets the loudness.
  - When it stops driving, an ending plays (the root and the chord on the next 1, lasting 2 beats), and then silence.
  - A tap after silence plays the root and the chord at once (pickup), so the tap clearly causes the sound. The pattern starts on the next sixteenth after it.
- **Levels 3–4:**
  - Level 3: the player's right-hand chord sounds as played. midzer plays only the pattern's **bass line**, following the heard chord. The tap rate counts the player's chord strikes, with notes within 0.1 beat counting as one chord, as in Solo's `CHORD_BEATS`.
  - Level 4: midzer plays nothing. It sends the pattern's next note as a cue (`AccompCue`), about a beat ahead, and the player's notes are judged as in Chords mode (`strike_note`).
- **Routing (`engine.rs`):**
  - Mode Accompany (`accomp.level() > 0`, `solo.level() == 0`, live, keys): every NoteOn and NoteOff goes to `accomp.press` or `accomp.release`, returning `Pressed` like `Solo`.
  - Solo with the left hand (`accomp.split() > 0`): keys below the split go to `Accomp`, and keys at or above it go to `Solo`. Accompany's split is its own setting. It is not the Jam `SetSplit` and must not change `self.split`.
- **Channel:** Accompany plays on a new `ACCOMP_CHANNEL` = 5, so its keys never cut the solo's. `SetSound` sends the `ProgramChange` to both `PLAYER_CHANNEL` and `ACCOMP_CHANNEL`. Check that 5 is free in `style.rs` (band channels 1–3), `melody.rs` (4), and `event.rs` (9, 14, 15).
- **Protocol** (mirror each item in `command.rs`/`event.rs` with round trips, and in `packages/audio-web/src/protocol.ts` with `protocol.test.ts`). Check the highest op first; it was 57 for commands and 22 for events on 2026-10-25.
  - `Command::SetAccomp { level: u8 (0 = off), pattern: u8, split: u8 (0 = all keys), lead_ms }`, op 58.
  - `Command::AccompChord { slot: Slot }`, op 59, encoded with `slot_from` like `SongChord`. The UI sends the chord at ms + `SOLO_SWITCH_MS`, only when it changes.
  - `EngineEvent::AccompFeel { energy: u8 }`, op 23, at most 10 per second, for its Feel meter.
  - `EngineEvent::AccompCue { key: Option<u8> }`, op 24, for level 4.
- **No allocation:** `Accomp` uses only fixed arrays (for the sounding notes, up to 8 at once, with their off-frames) and the static patterns.

## UI
- **`playalong/accomp.ts`** (with a test):
  - `ACCOMP_LEVELS` (name and detail), `ACCOMP_PATTERNS` (name and detail), `parseAccompLevel`, `parseAccompPattern`;
  - `accompHints(level, chord, split)`: levels 1–2 light the chord's root (`TAP_HINT`) at `RIGHT_C + root` alone, or at `48 + root` with the split; level 3 lights the level 3 close voicing (or the split register's); level 4 lights the cue key.
- **`session.ts`:**
  - `alongMode` gains `'accompany'`. `parseAlongMode` keeps old prefs working, and on drums `'accompany'` acts as `'chords'`.
  - New prefs: `accompLevel`, `accompPattern`, `soloLeftHand: boolean`.
  - `sendAccomp()` mirrors `sendSolo()`: it sends level 0 unless accompanying, and split 60 when `soloLeftHand` is on in Solo. It is called at engine start, on the setters, and on `setInstrument`.
  - The chord goes out from `followLiveChords`.
  - New state `accompFeel`, and `accompCue` from the event.
  - The space bar taps the lit root at levels 1–2 (see `spaceTap`).
- **`screens/PlayAlong.tsx`:**
  - `ModePicker` shows `Chords | Accompany | Solo` on keys, and still `Along | Solo` on drums.
  - In Accompany, the panel has `AccompLevelPicker` and `PatternPicker`. The stage keeps the same view as Chords (`AlongChords`: `ChordLanes` and `Keyboard`) with a small Feel meter and a prompt in the head.
  - `ChordLanes` takes a `level` override, so it can draw the keys the player presses at each Accompany level, and a register shift for the split.
  - In Solo, an "Accompany with the left hand" toggle appears, and with it on, the accompany level and pattern pickers.
  - `AlongTiming` hides at Accompany levels 1–3.

## Slices (each one builds, passes its tests, and is committed by the user)
1. **Solo keeps the chord view.** UI only:
   - in Solo on keys, the stage shows the chord lanes at the chord level above the keyboard, with the solo's prompt, Feel, call status and fit in the panel head;
   - the keyboard keeps the solo's lit keys;
   - the chord `LevelPicker` shows in Solo too, labelled "Chord guide";
   - e2e updated.
2. **Engine Accompany, levels 1–2:**
   - engine: `taps.rs` (extracted from Solo, no behaviour change), `accomp.rs` with the 5 patterns × 3 variations, `ACCOMP_CHANNEL`, `SetAccomp`, `AccompChord` and `AccompFeel`;
   - UI: the third mode, the level and pattern pickers, hints, prefs, Feel, and space-bar taps.
3. **Accompany levels 3–4:** own chords with the bass line, the cue with `AccompCue`, judging at level 4, and `AlongTiming`.
4. **Left hand in Solo:**
   - the split routing in `engine.rs`, the toggle, the lanes in the left-hand register, and both Feel meters;
   - the accompaniment goes into the solo take: `SoloNote` gains an `accomp` flag, `ReplayNote` plays it on `ACCOMP_CHANNEL`, and `SoloRoll` gives it its own colour.
5. **Docs:** the Status section of `docs/PLAN.md` and the architecture memory.

## Verification
- **Rust** (`cargo test -p midzer-core --lib`; never run `cargo fmt` on whole files). In `accomp.rs`, use a fixed seed and the synthetic 120 BPM grid from `solo.rs`'s tests:
  - no taps give no notes;
  - one tap per bar keeps variation A going for 2 bars after the last tap, and then the ending plays and it goes silent;
  - faster taps move to B and then C, switching only on a 1;
  - every note is a chord tone of the current chord (bass tones per the pattern), and it re-voices on a chord change;
  - level 2 runs while held and ends after the release;
  - level 3 plays only the bass line and passes the player's notes through;
  - level 4 plays nothing and cues the next note;
  - nothing allocates.
  - `engine.rs`: Accompany takes the keys over live audio; with Solo and the split, keys route by the split; `SetSound` reaches both channels.
  - All `solo.rs` tests pass unchanged after the `taps.rs` move.
- **TS:** `pnpm -C apps/web test` (`accomp.test.ts`, `protocol.test.ts`) and `pnpm typecheck`.
- **e2e:** rebuild the wasm and the web app first (`pnpm build:wasm`, then `pnpm --filter @midzer/web build`). In `along.spec.ts`:
  - Solo shows the chord lanes;
  - Accompany at level 1: space-bar taps, the Feel rises, and keys light.
  - The along sync, spread and learn failures are known and not regressions.
- **By ear:** with the user's mp3s, try each pattern at levels 1–2, and then Solo with the left hand. The pattern lines and the energy thresholds get tuned here.

## Handoff notes (read before starting the next slice)

### Slice 1: done (built 2026-10-25, to be committed by the user)
**UI only (`apps/web/src/screens/PlayAlong.tsx`, `index.css`, `e2e/along.spec.ts`)**
- `KeysSoloPlay` now shows, top to bottom:
  - the head: "Solo · key" and the prompt;
  - `div.solo-feel.compact` (one row: the Feel word or `CallStatus`, the meter, and `SoloFit` from level 3);
  - a second `along-play-head.solo-guide` with "Chords" and the `HandLegend`;
  - `ChordLanes`, then the `Keyboard`.
- The keyboard's lit keys are still the solo's (`soloHints` in `followLiveChords`, `session.ts`). The lanes come from `ChordLanes`, which reads `alongLevel` and the chord window by itself, so nothing in `session.ts` changed. Every key still drives the solo; the chords aren't judged.
- `HandLegend` (new, shared with `AlongChords`) shows the Left hand / Right hand / Next chord swatches. The left hand shows only at chord levels 4–5.
- `LevelPicker` takes a `label`: it is "Level" in Chords and "Chord guide" in Solo on keys. Both use the group id `along-level-label` and the testid `along-level`. In Solo the panel shows `SoloLevelPicker` first, then the chord guide. On drums neither shows (as before).
- CSS: `.solo-feel.compact` is a single flex row, with the word at `clamp(1.25rem, 3vw, 1.75rem)` and the meter flex 1, at least 120 px wide. `.along-play-head.solo-guide` has no top padding. `DrumSoloPlay` keeps the big, centred Feel.
- e2e: the keys solo test expects `chord-lanes` visible in Solo, picks chord guide 2, and expects "Three keys".

**Checks run:** `pnpm typecheck`, web unit tests (179). After `pnpm build:wasm` and the web build, both solo e2e tests pass. The screenshot (`MIDZER_SHOTS=<dir>`, `17-play-along-solo.png`) shows the Feel row, the lanes and the lit home key.

**Open after slice 1:** not tried by ear or by eye on a phone. With a narrow stage, the lanes share the height with the Feel row, and `.jam-stage .along-play` keeps `min-height: 340px`.

### Slice 2: done (built 2026-10-25, to be committed by the user)
**Engine (`crates/midzer-core`)**
- `taps.rs`: `Taps { rate, last_tap, touch }` with `tap()` (Solo's rules), `count(velocity, now, period, chords, gap, first)` (a custom gap and first-tap rate), `gap()`, `recent()`, `clear()`; plus `energy_of` (Solo's rate → energy) and `follow(energy, target, beats)` (the rise/fall smoothing). `Solo` holds `taps: Taps`; its behaviour is unchanged (all solo tests pass as before). `Grid::frame`/`beat_in_bar` and `Slot::chord` are now `pub(crate)`.
- `accomp.rs`: `Accomp` with `set(level, pattern, split, lead_ms)`, `set_chord(Option<Chord>)`, `press` (returns `solo::Pressed`), `release`, `silence`, `process`. `ACCOMP_CHANNEL` = 5 (exported from `lib.rs`), `MAX_ACCOMP_LEVEL` = **2 for now** (slice 3 raises it to 4; the decoder and the TS clamp follow it), `PATTERNS` = 5.
  - Patterns: `LINES` table, (bass, chord) grid strings per variation; an empty string inherits the previous variation's line. Built once in `patterns()` (`OnceLock`), forced in `Accomp::new` so the audio path never builds them.
  - Accompany's own energy map (`accomp::energy_of`: 0.25 taps/beat → 0.15, 1 → 0.5, 2+ → 0.85, piecewise in log2) and `variation_of(energy, current)` with B from 0.4, C from 0.75, ±0.05 hysteresis; switched only on a bar's 1.
  - Taps use `Taps::count(.., gap = KEEP_BEATS (8), first = 0.25)`, so a tap a bar stays calm (A). Level 1 is driven while the last tap is ≤ 8 beats old; level 2 while the root is held.
  - Driving keys: the chord's root in any octave, the last chord's root within 0.5 beat after a change, or any key before the first chord. Other keys are silent (`Pressed::Drove`).
  - Pickup: a driving tap while not running plays bass root + chord voicing at once (1 beat long); pattern steps within 1/4 beat after it are skipped. Ending: on the first 1 where it's not driven, root + chord for 2 beats, then silent.
  - Bass: `bass_key` puts the root at 36 + root (C2–B2), tones by `voicing::bass_note` offsets. Chord: `voice_near` around 60 (52–72), or 52 (45–59) when `split > 0`, re-voiced on each chord change, drifting home (0.7/0.3 like the band). `ChordNote(i)` past the top goes up an octave.
  - A chord change ends all sounding notes at once. Losing the grid ends everything.
  - 8 fixed sounding slots; nothing allocates after `new`.
- `engine.rs`: `accompany = !drums && live && !solo && accomp.level() > 0` routes NoteOn/NoteOff to `Accomp` (a driving press starts the live run like Solo). `accomp.silence` on AllNotesOff, Stop, LiveReset and ReplayPlay. `accomp.process` runs next to `solo.process`. `sound_changes()` sends the program to `PLAYER_CHANNEL` and `ACCOMP_CHANNEL` (on SetSound and Play). Solo with a split is **not** routed yet (slice 4); with both levels set, Solo wins.
- Protocol: `SetAccomp` op 58 (`[op, level | pattern << 8 | split << 16, lead_ms bits, 0]`), `AccompChord` op 59 (`[op, kind, packed, 0]`, `slot_words`/`slot_from`), event `AccompFeel` op 23. `AccompCue` (op 24) is **not** added yet (slice 3). midzer-synth ignores `AccompFeel`.
- Tests: `accomp.rs` (silence without taps; a tap a bar stays on A, keeps going, ends on the right 1; faster taps go B → C only on a 1; chord tones only, re-voice, nothing rings over a change, bass follows; only the root drives; level 2 runs while held, a short press ends on the next 1; energy map and hysteresis; every line parses on the sixteenth grid). `engine.rs`: `an_accompaniment_takes_the_keys_over_live_audio` (via a new `over_clicks(instrument, keys, seconds, setup)` helper that `solo_over_clicks_on` now uses); the SetSound test checks channel 5 too.

**UI**
- `packages/audio-web`: `setAccomp`/`accompChord` commands, `accompFeel` event (`protocol.ts`, a shared `slotWords` now also used by `songChord`), `ACCOMP_CHANNEL` in `channels.ts` (`partOfChannel` returns null for it: not drawn on the keys).
- `playalong/accomp.ts` (+ test): `ACCOMP_LEVELS` (1 Tap the root, 2 Hold the root), `ACCOMP_PATTERNS` (ids arpeggio, broken, stabs, ballad, bounce; index = engine pattern), `parseAccompLevel`, `parseAccompPattern`, `patternIndex`, `accompRoot(chord, split)` (60 + root, or 48 + root with a split), `accompHints(level, chord, split)`.
- `playalong/solo.ts`: `AlongMode` is `'chords' | 'accompany' | 'solo'`; `parseAlongMode` keeps `'accompany'`.
- `session.ts`: state/prefs `accompLevel`, `accompPattern`; state `accompFeel`; `setAccompLevel`, `setAccompPattern`; `accompanying()` (mode accompany and keys); `sendAccomp()` (split 0 for now) called at engine start, on `setAlongMode`, the setters and `setInstrument`. `sendAccompChord` in `followLiveChords` sends the chord at ms + `SOLO_SWITCH_MS` **only when a chord is heard**: gaps between chord spans keep the last chord (they flicker to "no chord" often; sending nc cut the notes and lost the root to tap). `forgetLiveSong` sends nc once. Hints: `accompHints` when accompanying. Space bar: `spaceTap()` gives the accompaniment's root at levels 1–2; the key pressed is kept in `spaceHeld` and released on keyup even if the chord changed (level 2 holds).
- `ChordLanes` takes a `level` override. `PlayAlong.tsx`: `ModePicker` shows Chords | Accompany | Solo on keys (Along | Solo on drums, where accompany shows as Along); `AccompPlay` (head "Accompany · pattern" + `accomp-prompt`, compact Feel row via the new `FeelMeter`/`FeelTrack` shared with `SoloFeel`, "Roots" legend with `HandLegend oneHand`, `ChordLanes level={1}`, keyboard); panel shows `AccompLevelPicker` (group "Accompany level", testid `accomp-level`) and `PatternPicker` (select, testid `accomp-pattern`) instead of the chord `LevelPicker`; `AlongTiming` hides while accompanying.
- e2e `along.spec.ts`: "accompanying by tapping the chord's root" (Accompany, pattern Pop stabs, Listen, wait for a chord, 24 space taps → Feel Burning/On fire, no timing report, prefs saved). Screenshot `21-play-along-accompany.png`.

**Checks run:** `cargo test -p midzer-core --lib` (187), clippy on core and synth clean, `pnpm typecheck`, web unit tests (181), audio-web tests (35); after `pnpm build:wasm` and the web build, `along.spec.ts` passes except the known "follows the beat" test.

**Open after slice 2:** nothing tried by ear. The pattern lines, the velocities (`velocity()` scales a step by 0.45 + 0.55 × touch/100), the pickup length and the energy thresholds are untuned. A chord change ends the bass mid-bar for long-bass patterns (Ballad A) until the next bass step; consider re-striking the bass on a change if it sounds empty.

### Notes for slice 3 (Accompany levels 3–4)
- Raise `MAX_ACCOMP_LEVEL` to 4 and add levels 3–4 to `ACCOMP_LEVELS`; `accompHints` level 3 should light the close voicing (`voicing(chord, 3)` from `levels.ts`, or the split register's), level 4 the cue key.
- Level 3: in `Accomp::press`, sound the player's own notes on `PLAYER_CHANNEL` from `Accomp` itself (as Solo's `own_note` does) and return `Pressed::Played(key)` (the accompany branch in `engine.rs` then needs Solo's `Played` arms: `strike_note` when judged), count chord strikes with `taps.count(.., chords = true, ..)` (`CHORD_BEATS` = 0.1), and play only `line.bass` in `sixteenth`.
- Level 4: no pattern notes; push `AccompCue { key: Option<u8> }` (event op 24) about a beat ahead from the pattern's next step, and judge with `strike_note` like Solo's `judged()` arm. `AlongTiming` should then show at level 4 (it hides for all accompany levels now).
- `ChordLanes level` for levels 3–4 should be 3 (close voicing), not 1.
