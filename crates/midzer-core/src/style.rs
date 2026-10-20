//! Arranger styles: per-part one-bar patterns written relative to the current chord.
//!
//! M1 ships one built-in style as static data; later milestones load styles
//! from files authored as MIDI.

use crate::event::DRUM_CHANNEL;

/// Ticks per quarter note.
pub const PPQ: u32 = 480;
pub const BEATS_PER_BAR: u32 = 4;
pub const TICKS_PER_BAR: u32 = PPQ * BEATS_PER_BAR;

/// A chord-relative bass tone.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tone {
    Root,
    Third,
    Fifth,
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
}

#[derive(Debug, Clone, Copy)]
pub struct Step {
    pub tick: u32,
    pub dur: u32,
    pub velocity: u8,
    pub hit: Hit,
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
    /// One bar, sorted by tick.
    pub steps: &'static [Step],
}

pub struct Style {
    pub name: &'static str,
    pub default_bpm: f32,
    pub parts: &'static [Part],
}

const fn s(tick: u32, dur: u32, velocity: u8, hit: Hit) -> Step {
    Step { tick, dur, velocity, hit }
}

const KICK: Hit = Hit::Drum(36);
const SNARE: Hit = Hit::Drum(38);
const HAT: Hit = Hit::Drum(42);
const E: u32 = PPQ / 2; // eighth note

pub static POP: Style = Style {
    name: "Pop",
    default_bpm: 100.0,
    parts: &[
        Part {
            kind: PartKind::Drums,
            channel: DRUM_CHANNEL,
            program: 0,
            steps: &[
                s(0, E, 110, KICK),
                s(0, E, 90, HAT),
                s(E, E, 60, HAT),
                s(2 * E, E, 105, SNARE),
                s(2 * E, E, 85, HAT),
                s(3 * E, E, 95, KICK),
                s(3 * E, E, 60, HAT),
                s(4 * E, E, 105, KICK),
                s(4 * E, E, 90, HAT),
                s(5 * E, E, 60, HAT),
                s(6 * E, E, 110, SNARE),
                s(6 * E, E, 85, HAT),
                s(7 * E, E, 65, HAT),
            ],
        },
        Part {
            kind: PartKind::Bass,
            channel: 2,
            program: 33, // Electric Bass (finger)
            steps: &[
                s(0, 3 * E - 40, 100, Hit::Bass(Tone::Root)),
                s(3 * E, E - 30, 80, Hit::Bass(Tone::Root)),
                s(4 * E, 2 * E - 40, 95, Hit::Bass(Tone::Fifth)),
                s(6 * E, E - 30, 85, Hit::Bass(Tone::Octave)),
                s(7 * E, E - 30, 80, Hit::Bass(Tone::Root)),
            ],
        },
        Part {
            kind: PartKind::Keys,
            channel: 1,
            program: 0, // Acoustic Grand Piano
            steps: &[
                s(0, 4 * E - 60, 80, Hit::Chord),
                s(4 * E, 3 * E - 60, 72, Hit::Chord),
                s(7 * E, E - 20, 64, Hit::Chord),
            ],
        },
        Part {
            kind: PartKind::Pad,
            channel: 3,
            program: 89, // Pad 2 (warm)
            steps: &[s(0, TICKS_PER_BAR, 55, Hit::Chord)],
        },
    ],
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn patterns_fit_in_one_bar_and_are_sorted() {
        for part in POP.parts {
            assert!(part.steps.windows(2).all(|w| w[0].tick <= w[1].tick), "{:?}", part.kind);
            for step in part.steps {
                assert!(step.tick < TICKS_PER_BAR && step.dur > 0 && step.velocity <= 127);
                assert!(step.tick + step.dur <= TICKS_PER_BAR);
            }
        }
    }
}
