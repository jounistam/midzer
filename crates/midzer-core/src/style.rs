//! Arranger styles: per-part patterns for each section, written relative to
//! the current chord.
//!
//! Patterns are authored as step grids (see [`drums`], [`bass`], [`chords`])
//! and built once, the first time [`styles`] is called. Everything after that
//! only reads them, so the audio path never allocates.

use std::sync::OnceLock;

use crate::event::DRUM_CHANNEL;

/// Ticks per quarter note.
pub const PPQ: u32 = 480;
pub const BEATS_PER_BAR: u32 = 4;
pub const TICKS_PER_BAR: u32 = PPQ * BEATS_PER_BAR;

/// Number of main variations (A, B, C).
pub const VARIATIONS: usize = 3;

pub const KICK: u8 = 36;
pub const SIDE_STICK: u8 = 37;
pub const SNARE: u8 = 38;
pub const HAT: u8 = 42;
pub const OPEN_HAT: u8 = 46;
pub const CRASH: u8 = 49;
pub const RIDE: u8 = 51;
pub const TAMBOURINE: u8 = 54;
pub const COWBELL: u8 = 56;
pub const SHAKER: u8 = 70;
pub const WOOD_BLOCK: u8 = 76;
pub const TOM_LOW: u8 = 45;
pub const TOM_MID: u8 = 48;
pub const TOM_HIGH: u8 = 50;

/// A chord-relative bass tone.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tone {
    Root,
    Third,
    Fifth,
    /// The major sixth.
    Sixth,
    /// The chord's seventh, or a minor seventh for triads.
    Seventh,
    Octave,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Hit {
    /// A fixed General MIDI drum key.
    Drum(u8),
    /// One bass tone of the current chord.
    Bass(Tone),
    /// The part's full voicing of the current chord.
    Chord,
    /// One note of the voicing, counted from the bottom; indexes past the top
    /// continue an octave higher. For arpeggios.
    ChordNote(u8),
}

#[derive(Debug, Clone, Copy)]
pub struct Step {
    pub tick: u32,
    pub dur: u32,
    pub velocity: u8,
    pub hit: Hit,
}

/// One or more bars of steps, sorted by tick.
#[derive(Debug, Clone, Default)]
pub struct Pattern {
    pub bars: u32,
    pub steps: Vec<Step>,
}

impl Pattern {
    /// The steps of the bar that plays as song bar `bar`, with ticks relative
    /// to that bar's start.
    pub fn bar(&self, bar: u64) -> impl Iterator<Item = Step> + '_ {
        let offset = (bar % self.bars.max(1) as u64) as u32 * TICKS_PER_BAR;
        self.steps
            .iter()
            .filter(move |s| s.tick >= offset && s.tick < offset + TICKS_PER_BAR)
            .map(move |s| Step { tick: s.tick - offset, ..*s })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PartKind {
    Drums,
    Bass,
    Keys,
    Pad,
}

pub struct Part {
    pub kind: PartKind,
    pub channel: u8,
    pub program: u8,
    pub intro: Pattern,
    pub main: [Pattern; VARIATIONS],
    /// `None` keeps playing the current main variation during fills.
    pub fill: Option<Pattern>,
    pub ending: Pattern,
    /// What the part plays while the band waits for the player; `None` rests.
    pub rest: Option<Pattern>,
}

pub struct Style {
    pub name: &'static str,
    pub default_bpm: f32,
    pub parts: Vec<Part>,
}

impl Style {
    /// Whether the grooves are shuffled: drum steps off the sixteenth grid.
    pub fn swung(&self) -> bool {
        self.parts.iter().filter(|p| p.kind == PartKind::Drums).flat_map(|p| p.main[0].steps.iter()).any(|s| s.tick % (PPQ / 4) != 0)
    }
}

/// The built-in styles. Order is part of the protocol: the UI selects by index
/// (see `STYLES` in packages/audio-web/src/protocol.ts).
pub fn styles() -> &'static [Style] {
    static STYLES: OnceLock<Vec<Style>> = OnceLock::new();
    STYLES.get_or_init(|| vec![pop(), rock(), funk(), bossa(), ballad(), blues()])
}

// ---- Grid notation --------------------------------------------------------
//
// One character per grid step; `|` separates bars and spaces are ignored. The
// grid resolution is the number of steps in a bar, so 16 characters are
// sixteenths and 12 are shuffle triplets. `-` holds the previous note and `.`
// rests.

/// Splits a grid into its characters, the number of bars and ticks per step.
fn grid(text: &str) -> (Vec<char>, u32, u32) {
    let bars = text.split('|').count() as u32;
    let cells: Vec<char> = text.chars().filter(|c| !matches!(c, '|' | ' ')).collect();
    let per_bar = cells.len() as u32 / bars;
    assert!(per_bar > 0 && per_bar * bars == cells.len() as u32, "uneven bars in grid {text:?}");
    assert!(TICKS_PER_BAR.is_multiple_of(per_bar), "grid {text:?} does not divide the bar");
    (cells, bars, TICKS_PER_BAR / per_bar)
}

/// Parses a grid with `symbol` mapping a character to a velocity and hit.
fn parse(text: &str, symbol: impl Fn(char) -> Option<(u8, Hit)>) -> Pattern {
    let (cells, bars, step) = grid(text);
    let mut steps = Vec::new();
    for (i, &c) in cells.iter().enumerate() {
        if matches!(c, '-' | '.') {
            continue;
        }
        let (velocity, hit) = symbol(c).unwrap_or_else(|| panic!("unknown symbol {c:?} in grid {text:?}"));
        let held = cells[i + 1..].iter().take_while(|&&c| c == '-').count() as u32;
        let tick = i as u32 * step;
        // Notes on the beat lean in a little; a small gap keeps repeats articulate.
        let accent = if tick.is_multiple_of(TICKS_PER_BAR) { 6 } else if tick.is_multiple_of(PPQ) { 0 } else { -6 };
        steps.push(Step {
            tick,
            dur: (held + 1) * step - (step / 8).min(30),
            velocity: (velocity as i32 + accent).clamp(1, 127) as u8,
            hit,
        });
    }
    Pattern { bars, steps }
}

fn drum_velocity(c: char) -> Option<u8> {
    match c {
        'X' => Some(118),
        'x' => Some(96),
        'o' => Some(70),
        'g' => Some(42),
        _ => None,
    }
}

/// A drum pattern from one grid line per drum: `X` accent, `x` normal,
/// `o` soft, `g` ghost.
pub fn drums(lines: &[(u8, &str)]) -> Pattern {
    let mut pattern = Pattern { bars: 1, steps: Vec::new() };
    for &(key, line) in lines {
        let part = parse(line, |c| drum_velocity(c).map(|v| (v, Hit::Drum(key))));
        pattern.bars = pattern.bars.max(part.bars);
        // Drums ring out on their own; a short note-off is enough.
        pattern.steps.extend(part.steps.into_iter().map(|s| Step { dur: s.dur.min(PPQ / 4), ..s }));
    }
    pattern.steps.sort_by_key(|s| s.tick);
    pattern
}

/// A bass line: `R` root, `3` third, `5` fifth, `6` sixth, `7` seventh,
/// `8` octave.
pub fn bass(line: &str) -> Pattern {
    parse(line, |c| {
        let tone = match c {
            'R' => Tone::Root,
            '3' => Tone::Third,
            '5' => Tone::Fifth,
            '6' => Tone::Sixth,
            '7' => Tone::Seventh,
            '8' => Tone::Octave,
            _ => return None,
        };
        Some((92, Hit::Bass(tone)))
    })
}

/// A chord part: `X` accented, `x` normal and `o` soft strikes of the full
/// voicing; `1`–`8` play single voicing notes from the bottom.
pub fn chords(line: &str) -> Pattern {
    parse(line, |c| match c {
        'X' => Some((94, Hit::Chord)),
        'x' => Some((78, Hit::Chord)),
        'o' => Some((60, Hit::Chord)),
        '1'..='8' => Some((70, Hit::ChordNote(c as u8 - b'1'))),
        _ => None,
    })
}

const BASS_CHANNEL: u8 = 2;
const KEYS_CHANNEL: u8 = 1;
const PAD_CHANNEL: u8 = 3;

fn drum_part(kit: u8, intro: Pattern, main: [Pattern; VARIATIONS], fill: Pattern) -> Part {
    Part {
        kind: PartKind::Drums,
        channel: DRUM_CHANNEL,
        program: kit,
        intro,
        main,
        fill: Some(fill),
        ending: drums(&[(KICK, "X..............."), (CRASH, "X...............")]),
        rest: None,
    }
}

/// A chord-following part whose intro is variation A and whose ending is one
/// long final note.
fn harmonic_part(kind: PartKind, program: u8, main: [Pattern; VARIATIONS]) -> Part {
    let (channel, build, ending): (u8, fn(&str) -> Pattern, &str) = match kind {
        PartKind::Bass => (BASS_CHANNEL, bass, "R---------------"),
        PartKind::Keys => (KEYS_CHANNEL, chords, "X---------------"),
        _ => (PAD_CHANNEL, chords, "x---------------"),
    };
    Part {
        kind,
        channel,
        program,
        intro: main[0].clone(),
        main,
        fill: None,
        ending: build(ending),
        rest: (kind == PartKind::Pad).then(|| chords("o---------------")),
    }
}

// ---- Built-in styles --------------------------------------------------------

fn pop() -> Style {
    Style {
        name: "Pop",
        default_bpm: 100.0,
        parts: vec![
            drum_part(
                0,
                drums(&[(KICK, "x.......x......."), (HAT, "x.x.x.x.x.x.x.x."), (SNARE, "........x.x.xxxx")]),
                [
                    drums(&[(KICK, "x.....x.x......."), (SNARE, "....x.......x..."), (HAT, "x.o.x.o.x.o.x.o.")]),
                    drums(&[
                        (KICK, "x.....x.x.x....."),
                        (SNARE, "....x.......x..g"),
                        (HAT, "x.o.x.o.x.o.x..."),
                        (OPEN_HAT, "..............o."),
                    ]),
                    drums(&[
                        (KICK, "x.....x.x.x...x."),
                        (SNARE, "....X..g....X.g."),
                        (HAT, "xoxoxoxoxoxoxoxo"),
                        (TAMBOURINE, "....x.......x..."),
                    ]),
                ],
                drums(&[
                    (KICK, "x.......x......."),
                    (HAT, "x.o.x.o........."),
                    (SNARE, "....x...x.x.x.xx"),
                    (TOM_MID, "...........x.x.."),
                    (TOM_LOW, "...............x"),
                ]),
            ),
            harmonic_part(
                PartKind::Bass,
                33, // Electric Bass (finger)
                [bass("R-----R-5---8-R-"), bass("R--R--R-5--5--8-"), bass("R-.RR-.R5-.58-5-")],
            ),
            harmonic_part(
                PartKind::Keys,
                0, // Acoustic Grand Piano
                [chords("x-------x-----o-"), chords("x--ox-o-x--ox-o-"), chords("X-ox-ox-x-ox-oxo")],
            ),
            harmonic_part(
                PartKind::Pad,
                89, // Pad 2 (warm)
                [chords("o---------------"), chords("x---------------"), chords("x-------x-------")],
            ),
        ],
    }
}

fn rock() -> Style {
    Style {
        name: "Rock",
        default_bpm: 120.0,
        parts: vec![
            drum_part(
                16, // Power kit
                drums(&[(KICK, "x.......x......."), (HAT, "x.x.x.x.x.x.x.x."), (SNARE, "........XxXxXXXX")]),
                [
                    drums(&[(KICK, "x.......x.x....."), (SNARE, "....X.......X..."), (HAT, "x.x.x.x.x.x.x.x.")]),
                    drums(&[(KICK, "x.....x.x.x....."), (SNARE, "....X.......X..."), (RIDE, "x.x.x.x.x.x.x.x.")]),
                    drums(&[
                        (KICK, "x.x...x.x.x...x."),
                        (SNARE, "....X.......X..."),
                        (OPEN_HAT, "x.x.x.x.x.x.x.x."),
                        (CRASH, "x..............."),
                    ]),
                ],
                drums(&[
                    (KICK, "x.......x...x..."),
                    (HAT, "x.x.x.x........."),
                    (SNARE, "....X...xxxx...."),
                    (TOM_MID, "............xx.."),
                    (TOM_LOW, "..............xx"),
                ]),
            ),
            harmonic_part(
                PartKind::Bass,
                34, // Electric Bass (pick)
                [bass("R-R-R-R-R-R-R-R-"), bass("R-R-R-R-5-5-R-R-"), bass("RRR-RRR-RRR-5-8-")],
            ),
            harmonic_part(
                PartKind::Keys,
                29, // Overdriven Guitar
                [chords("x-------x-------"), chords("x.x.x.x.x.x.x.x."), chords("X.x.x.x.X.x.x.x.")],
            ),
            harmonic_part(
                PartKind::Pad,
                16, // Drawbar Organ
                [chords("o---------------"), chords("o---------------"), chords("x-------x-------")],
            ),
        ],
    }
}

fn funk() -> Style {
    Style {
        name: "Funk",
        default_bpm: 104.0,
        parts: vec![
            drum_part(
                0,
                drums(&[(HAT, "xoxoxoxoxoxoxoxo"), (SNARE, "....x.......x.xx")]),
                [
                    drums(&[(KICK, "x..x..x...x..x.."), (SNARE, "....x..g.g..x..g"), (HAT, "xoxoxoxoxoxoxoxo")]),
                    drums(&[
                        (KICK, "x..x..x...x..x.."),
                        (SNARE, "....x..g.g..x..g"),
                        (HAT, "xoxoxoxoxoxoxo.o"),
                        (OPEN_HAT, "..............x."),
                    ]),
                    drums(&[
                        (KICK, "x..x..x.x.x..x.x"),
                        (SNARE, "....X.gg.g..X.gg"),
                        (HAT, "xxxxxxxxxxxxxxxx"),
                        (COWBELL, "x...x...x...x..."),
                    ]),
                ],
                drums(&[
                    (KICK, "x..x..x........."),
                    (HAT, "xoxoxoxo........"),
                    (SNARE, "....x..g..x.xxXx"),
                    (TOM_HIGH, "........x......."),
                    (TOM_MID, ".........x......"),
                ]),
            ),
            harmonic_part(
                PartKind::Bass,
                36, // Slap Bass 1
                [bass("R..R...8.R.5..R."), bass("R.8R..R.8.R.5.7."), bass("R8.RR.8R.R8.5.78")],
            ),
            harmonic_part(
                PartKind::Keys,
                7, // Clavi
                [chords(".x.x..x..x.x..x."), chords(".x.xx.x..x.xx.x."), chords("x.xx.xxx.x.xx.xx")],
            ),
            harmonic_part(
                PartKind::Pad,
                50, // Synth Strings 1
                [chords("................"), chords("o---------------"), chords("x-------x-------")],
            ),
        ],
    }
}

fn bossa() -> Style {
    Style {
        name: "Bossa",
        default_bpm: 140.0,
        parts: vec![
            drum_part(
                40, // Brush kit
                drums(&[(SIDE_STICK, "x..x..x...x..x.."), (SHAKER, "xgxgxgxgxgxgxgxg")]),
                [
                    drums(&[
                        (KICK, "o..oo..oo..oo..o"),
                        (SIDE_STICK, "x..x..x...x..x..|..x..x...x..x..."),
                        (SHAKER, "xgxgxgxgxgxgxgxg"),
                    ]),
                    drums(&[
                        (KICK, "o..oo..oo..oo..o"),
                        (SIDE_STICK, "x..x..x...x..x..|..x..x...x..x..."),
                        (RIDE, "o.o.o.o.o.o.o.o."),
                    ]),
                    drums(&[
                        (KICK, "x..xx..xx..xx..x"),
                        (SNARE, "x..x..x...x..x..|..x..x...x..x..."),
                        (RIDE, "x.o.x.o.x.o.x.o."),
                        (SHAKER, "xgxgxgxgxgxgxgxg"),
                    ]),
                ],
                drums(&[(KICK, "o..oo..o........"), (SNARE, "........x.x.xxx."), (TOM_LOW, "...............x")]),
            ),
            harmonic_part(
                PartKind::Bass,
                32, // Acoustic Bass
                [bass("R-------5-------"), bass("R-----5-5-------"), bass("R-----5-5-----5-")],
            ),
            harmonic_part(
                PartKind::Keys,
                24, // Acoustic Guitar (nylon)
                [chords("x..x..x...x..x.."), chords("x..x..x...x..x..|..x..x...x..x..."), chords("x.ox.ox.x.ox.ox.")],
            ),
            harmonic_part(
                PartKind::Pad,
                48, // String Ensemble 1
                [chords("................"), chords("o---------------"), chords("o-------o-------")],
            ),
        ],
    }
}

fn ballad() -> Style {
    Style {
        name: "Ballad",
        default_bpm: 72.0,
        parts: vec![
            drum_part(
                40, // Brush kit
                drums(&[(RIDE, "o...o...o...o..."), (TOM_LOW, "............o.o.")]),
                [
                    drums(&[(KICK, "x.......x......."), (SIDE_STICK, "....x.......x..."), (HAT, "o.o.o.o.o.o.o.o.")]),
                    drums(&[(KICK, "x.......x.x....."), (SNARE, "....x.......x..."), (HAT, "o.o.o.o.o.o.o.o.")]),
                    drums(&[
                        (KICK, "x.....x.x.x....."),
                        (SNARE, "....X.......X..."),
                        (RIDE, "x.x.x.x.x.x.x.x."),
                        (CRASH, "o..............."),
                    ]),
                ],
                drums(&[(KICK, "x..............."), (SNARE, "........x.x.x.x."), (TOM_MID, "........o.o....."), (TOM_LOW, "............o.oo")]),
            ),
            harmonic_part(
                PartKind::Bass,
                32, // Acoustic Bass
                [bass("R---------------"), bass("R-------5-------"), bass("R-----R-5---8-5-")],
            ),
            harmonic_part(
                PartKind::Keys,
                0, // Acoustic Grand Piano
                [chords("1-2-3-4-1-2-3-4-"), chords("x-2-3-4-x-2-3-4-"), chords("X-3-4-3-x-3-4-3-")],
            ),
            harmonic_part(
                PartKind::Pad,
                49, // String Ensemble 2
                [chords("o---------------"), chords("o---------------"), chords("x---------------")],
            ),
        ],
    }
}

fn blues() -> Style {
    Style {
        name: "Blues",
        default_bpm: 96.0,
        parts: vec![
            drum_part(
                0,
                drums(&[(HAT, "x.xx.xx.xx.x"), (SNARE, "......x.xxxx")]),
                [
                    drums(&[(KICK, "x.....x....."), (SNARE, "...x.....x.."), (HAT, "x.ox.ox.ox.o")]),
                    drums(&[(KICK, "x..o..x..o.."), (SNARE, "...x.....x.g"), (RIDE, "x.ox.ox.ox.o")]),
                    drums(&[(KICK, "x..x..x..x.."), (SNARE, "...X..g..X.g"), (RIDE, "x.xx.xx.xx.x"), (OPEN_HAT, "...x.....x..")]),
                ],
                drums(&[(KICK, "x.....x....."), (HAT, "x.ox.o......"), (SNARE, "......x.xxxx"), (TOM_LOW, "...........x")]),
            ),
            harmonic_part(
                PartKind::Bass,
                33, // Electric Bass (finger)
                [bass("R..5..R..5.."), bass("R.35.67.65.3"), bass("R.35.67.65.3|8.76.57.53.5")],
            ),
            harmonic_part(
                PartKind::Keys,
                0, // Acoustic Grand Piano
                [chords("...x.....x.."), chords("..x..x..x..x"), chords("x.xx.xx.xx.x")],
            ),
            harmonic_part(
                PartKind::Pad,
                16, // Drawbar Organ
                [chords("o-----------"), chords("o-----------"), chords("x-----x-----")],
            ),
        ],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn all_patterns(part: &Part) -> impl Iterator<Item = &Pattern> {
        [&part.intro, &part.ending].into_iter().chain(&part.main).chain(&part.fill).chain(&part.rest)
    }

    #[test]
    fn patterns_fit_their_bars_and_are_sorted() {
        for style in styles() {
            assert_eq!(style.parts.len(), 4, "{}", style.name);
            for part in &style.parts {
                for pattern in all_patterns(part) {
                    let len = pattern.bars * TICKS_PER_BAR;
                    assert!(pattern.steps.windows(2).all(|w| w[0].tick <= w[1].tick), "{} {:?}", style.name, part.kind);
                    for step in &pattern.steps {
                        assert!(step.tick < len && step.dur > 0 && step.velocity <= 127, "{} {:?}", style.name, part.kind);
                        assert!(step.tick + step.dur <= len, "{} {:?} runs past its pattern", style.name, part.kind);
                    }
                }
            }
        }
    }

    #[test]
    fn grids_set_resolution_and_length() {
        let p = bass("R--.5-.-");
        assert_eq!(p.bars, 1);
        assert_eq!(p.steps.len(), 2);
        assert_eq!((p.steps[0].tick, p.steps[1].tick), (0, 4 * 240));
        assert_eq!(p.steps[0].dur, 3 * 240 - 30);

        let shuffle = drums(&[(HAT, "x.xx.xx.xx.x")]);
        assert_eq!(shuffle.steps[1].tick, 2 * 160);

        let two = chords("x...|..x.");
        assert_eq!(two.bars, 2);
        assert_eq!(two.bar(1).map(|s| s.tick).collect::<Vec<_>>(), [2 * 480]);
        assert_eq!(two.bar(2).count(), 1);
    }

    #[test]
    fn style_list_matches_the_ui() {
        // packages/audio-web/src/protocol.ts `STYLES` mirrors this list.
        let list: Vec<_> = styles().iter().map(|s| (s.name, s.default_bpm as u32)).collect();
        assert_eq!(list, [("Pop", 100), ("Rock", 120), ("Funk", 104), ("Bossa", 140), ("Ballad", 72), ("Blues", 96)]);
    }

    #[test]
    fn variations_get_busier() {
        for style in styles() {
            let drums = &style.parts[0];
            let count = |p: &Pattern| p.steps.len() as f32 / p.bars as f32;
            assert!(count(&drums.main[0]) <= count(&drums.main[2]), "{}: C is not busier than A", style.name);
        }
    }
}
