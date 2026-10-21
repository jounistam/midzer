//! Drummer mode: the drummer leads the band. This module hears the count-in
//! that starts the band, the fills that move it to the next section, and
//! which hits keep time.

use crate::drums::Piece;
use crate::style::PPQ;

/// Hits a count-in takes.
pub const COUNT_IN_HITS: usize = 4;
/// Count-in tempo range, as seconds between hits.
const COUNT_IN_PERIOD_S: (f64, f64) = (60.0 / 240.0, 60.0 / 40.0);
/// How far one gap of a count-in may stray from their average.
const COUNT_IN_SLACK: f64 = 0.15;

/// Hits softer than this are ghost notes: they never keep time.
const GHOST_VELOCITY: u8 = 40;

/// What a count-in hit did.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CountIn {
    /// The hit wasn't on a count-in piece.
    Ignored,
    /// `hits` evenly spaced hits so far (1 to 3).
    Counting { hits: u8 },
    /// Four even hits: `period` frames per beat, and the frame where the
    /// fourth hit should have landed by their fit, so the next 1 is a period later.
    Done { period: f64, last: f64 },
}

/// Hears a count-in: four evenly spaced hits on one of the chosen pieces.
#[derive(Debug, Clone, Default)]
pub struct CountInListener {
    /// Count-in pieces as a bit set ([`Piece::bit`]); empty turns it off.
    pieces: u32,
    /// Frames of the current run of even hits, oldest first.
    times: [u64; COUNT_IN_HITS],
    len: usize,
}

impl CountInListener {
    pub fn set_pieces(&mut self, pieces: u32) {
        self.pieces = pieces;
        self.reset();
    }

    pub fn reset(&mut self) {
        self.len = 0;
    }

    /// Hears a hit at engine frame `frame`.
    pub fn hit(&mut self, piece: Piece, frame: u64, sample_rate: f32) -> CountIn {
        if self.pieces & piece.bit() == 0 {
            return CountIn::Ignored;
        }
        let sr = sample_rate as f64;
        if self.len == COUNT_IN_HITS {
            self.times.copy_within(1.., 0);
            self.len -= 1;
        }
        self.times[self.len] = frame;
        self.len += 1;
        // Keep the longest run of recent hits that is evenly spaced.
        while self.len > 1 && !even(&self.times[..self.len], sr) {
            self.times.copy_within(1..self.len, 0);
            self.len -= 1;
        }
        if self.len < COUNT_IN_HITS {
            return CountIn::Counting { hits: self.len as u8 };
        }
        // A least-squares line through the four hits: t = a + b·i.
        let mean_t = self.times.iter().map(|&t| t as f64).sum::<f64>() / COUNT_IN_HITS as f64;
        let mean_i = (COUNT_IN_HITS - 1) as f64 / 2.0;
        let (mut num, mut den) = (0.0, 0.0);
        for (i, &t) in self.times.iter().enumerate() {
            num += (i as f64 - mean_i) * (t as f64 - mean_t);
            den += (i as f64 - mean_i).powi(2);
        }
        let period = num / den;
        let last = mean_t + period * mean_i;
        self.reset();
        CountIn::Done { period, last }
    }
}

/// Whether the gaps between `times` are all close to their average, and the
/// average is a usable tempo.
fn even(times: &[u64], sr: f64) -> bool {
    let gaps = times.len() - 1;
    let mean = (times[gaps] - times[0]) as f64 / gaps as f64 / sr;
    if !(COUNT_IN_PERIOD_S.0..=COUNT_IN_PERIOD_S.1).contains(&mean) {
        return false;
    }
    times.windows(2).all(|w| ((w[1] - w[0]) as f64 / sr / mean - 1.0).abs() <= COUNT_IN_SLACK)
}

/// How much a hit tells about the tempo, on a beat and off it. Kick and snare
/// on the beat count most; cymbals that keep time less; toms and crashes,
/// which fills and accents are made of, and ghost notes not at all.
pub fn time_weight(piece: Piece, velocity: u8) -> (f32, f32) {
    if velocity < GHOST_VELOCITY {
        return (0.0, 0.0);
    }
    match piece {
        Piece::Kick | Piece::Snare | Piece::SnareRim | Piece::SideStick => (1.0, 0.3),
        Piece::HiHatClosed | Piece::HiHatOpen | Piece::HiHatPedal | Piece::Ride | Piece::RideBell => (0.5, 0.3),
        _ => (0.0, 0.0),
    }
}

/// Whether a hit is worth showing on the timing meter.
pub fn keeps_time(piece: Piece, velocity: u8) -> bool {
    time_weight(piece, velocity).0 > 0.0
}

/// Where fills are heard: the last two beats of a bar.
const FILL_FROM: f64 = 2.0 * PPQ as f64 - PPQ as f64 / 8.0;
/// A fill scores this much above the groove's usual score, and at least `FILL_MIN`.
const FILL_ABOVE: f32 = 3.5;
const FILL_MIN: f32 = 4.5;
/// Bars with hits heard before fills count, so the groove is known.
const FILL_WARM_UP_BARS: u32 = 2;

/// Hears fills: a burst of toms or snare late in the bar, well above what the
/// groove usually plays there.
#[derive(Debug, Clone, Copy, Default)]
pub struct FillDetector {
    bar: i64,
    score: f32,
    /// The groove's usual score in the last two beats, from bars without a fill.
    usual: f32,
    bars_heard: u32,
    fill: bool,
}

impl FillDetector {
    pub fn reset(&mut self) {
        *self = Self::default();
    }

    /// Hears a hit `in_bar` ticks into band bar `bar`. Returns true for the
    /// hit that makes the bar a fill.
    pub fn hit(&mut self, piece: Piece, velocity: u8, bar: i64, in_bar: f64) -> bool {
        if bar != self.bar {
            if !self.fill {
                // Quick to learn a busier groove, slow to forget it.
                let rate = if self.score > self.usual { 0.6 } else { 0.3 };
                self.usual += rate * (self.score - self.usual);
            }
            let bars_heard = self.bars_heard + 1;
            *self = Self { bar, usual: self.usual, bars_heard, ..Self::default() };
        }
        let weight = match piece {
            _ if velocity < GHOST_VELOCITY => 0.0,
            p if p.is_tom() => 1.5,
            Piece::Snare | Piece::SnareRim => 1.0,
            _ => 0.0,
        };
        if in_bar < FILL_FROM || weight == 0.0 {
            return false;
        }
        self.score += weight;
        if !self.fill && self.bars_heard >= FILL_WARM_UP_BARS && self.score >= FILL_MIN.max(self.usual + FILL_ABOVE) {
            self.fill = true;
            return true;
        }
        false
    }

    /// Whether band bar `bar` has a fill so far.
    pub fn in_fill(&self, bar: i64) -> bool {
        self.fill && self.bar == bar
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SR: f32 = 48_000.0;
    const HAT: u32 = 1 << 4;

    fn listener() -> CountInListener {
        let mut c = CountInListener::default();
        c.set_pieces(Piece::HiHatClosed.bit() | Piece::SideStick.bit());
        assert_eq!(Piece::HiHatClosed.bit(), HAT);
        c
    }

    #[test]
    fn four_even_hits_give_tempo_and_phase() {
        let mut c = listener();
        let mut results = Vec::new();
        for (i, frame) in [0u64, 24_100, 47_900, 72_000].into_iter().enumerate() {
            results.push(c.hit(if i == 2 { Piece::SideStick } else { Piece::HiHatClosed }, 1000 + frame, SR));
        }
        assert_eq!(&results[..3], &[CountIn::Counting { hits: 1 }, CountIn::Counting { hits: 2 }, CountIn::Counting { hits: 3 }]);
        let CountIn::Done { period, last } = results[3] else { panic!("{results:?}") };
        assert!((period - 23_980.0).abs() < 50.0, "{period}");
        assert!((last - 73_000.0).abs() < 100.0, "{last}");
    }

    #[test]
    fn other_pieces_and_uneven_hits_do_not_count() {
        let mut c = listener();
        assert_eq!(c.hit(Piece::Snare, 0, SR), CountIn::Ignored);
        c.hit(Piece::HiHatClosed, 0, SR);
        c.hit(Piece::HiHatClosed, 24_000, SR);
        // Twice as far: the run starts again from the last two hits only if they are even.
        assert_eq!(c.hit(Piece::HiHatClosed, 72_000, SR), CountIn::Counting { hits: 2 });
        assert_eq!(c.hit(Piece::HiHatClosed, 120_000, SR), CountIn::Counting { hits: 3 });
        // Too slow for a tempo: a lone hit.
        assert_eq!(c.hit(Piece::HiHatClosed, 300_000, SR), CountIn::Counting { hits: 1 });
        c.set_pieces(0);
        assert_eq!(c.hit(Piece::HiHatClosed, 324_000, SR), CountIn::Ignored);
    }

    #[test]
    fn a_fill_is_a_burst_late_in_the_bar() {
        let mut f = FillDetector::default();
        let beat = PPQ as f64;
        // Four bars of a rock groove: snare on 2 and 4, kick and hats.
        for bar in 0..4 {
            for eighth in 0..8 {
                let t = eighth as f64 * beat / 2.0;
                assert!(!f.hit(Piece::HiHatClosed, 90, bar, t));
                if eighth % 4 == 2 {
                    assert!(!f.hit(Piece::Snare, 100, bar, t));
                }
            }
        }
        // Sixteenth snares on beat 3 and toms on beat 4.
        let fill = [Piece::Snare, Piece::Snare, Piece::Snare, Piece::Snare, Piece::Tom1, Piece::Tom2, Piece::Tom3, Piece::Tom3];
        let heard: Vec<_> = fill.iter().enumerate().map(|(i, p)| f.hit(*p, 100, 4, 2.0 * beat + i as f64 * beat / 4.0)).collect();
        assert_eq!(heard.iter().filter(|h| **h).count(), 1);
        assert!(heard[4], "the first tom tips it: {heard:?}");
        assert!(f.in_fill(4));
        assert!(!f.in_fill(5));
        // Ghost notes and early hits don't count.
        assert!(!(0..12).any(|i| f.hit(Piece::Snare, 30, 6, 2.0 * beat + i as f64 * 40.0)));
        assert!(!(0..12).any(|i| f.hit(Piece::Tom1, 100, 7, i as f64 * 40.0)));
    }

    #[test]
    fn a_busy_groove_raises_the_bar_for_fills() {
        let mut f = FillDetector::default();
        let beat = PPQ as f64;
        // Sixteenth snares all bar long are the groove here.
        for bar in 0..8 {
            for i in 0..16 {
                f.hit(Piece::Snare, 90, bar, i as f64 * beat / 4.0);
            }
        }
        assert!(!f.in_fill(7), "the groove itself is no fill");
    }

    #[test]
    fn timekeeping_weights() {
        assert_eq!(time_weight(Piece::Kick, 100), (1.0, 0.3));
        assert_eq!(time_weight(Piece::Tom1, 100), (0.0, 0.0));
        assert_eq!(time_weight(Piece::Snare, 20), (0.0, 0.0));
        assert!(keeps_time(Piece::Ride, 80));
        assert!(!keeps_time(Piece::Crash, 127));
    }
}
