//! Song playback: a chord chart the band follows instead of the player's
//! chords, with section marks, a loop and a start position.
//!
//! The chart is loaded through commands (one chord slot per beat, one mark
//! per bar) into fixed arrays, so loading and playing never allocate. It can
//! be reloaded while playing; the band picks up the changes on the next beat.

use crate::chord::Chord;
use crate::conductor::Request;
use crate::style::BEATS_PER_BAR;

pub const MAX_SONG_BARS: usize = 512;
const BEATS: u32 = BEATS_PER_BAR;
const MAX_SONG_BEATS: usize = MAX_SONG_BARS * BEATS as usize;

/// What the chart says about the harmony on one beat.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Slot {
    /// Keep the chord that is playing.
    #[default]
    Hold,
    /// No chord: only the drums play.
    NoChord,
    Chord(Chord),
}

impl Slot {
    fn chord(self) -> Option<Option<Chord>> {
        match self {
            Slot::Hold => None,
            Slot::NoChord => Some(None),
            Slot::Chord(c) => Some(Some(c)),
        }
    }
}

/// What the song asks of the band on one beat.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SongBeat {
    /// The song beat being played.
    pub beat: u32,
    /// The song bar, on downbeats only.
    pub bar: Option<u32>,
    /// The harmony from this beat on; `None` keeps the current one.
    pub chord: Option<Option<Chord>>,
    /// A section change for the conductor, timed to land on the next bar.
    pub request: Option<Request>,
}

pub struct SongPlayer {
    slots: [Slot; MAX_SONG_BEATS],
    marks: [Option<Request>; MAX_SONG_BARS],
    bars: u32,
    /// Bars `start..end` repeat; `None` plays through to the ending.
    loop_bars: Option<(u32, u32)>,
    active: bool,
    start_bar: u32,
    /// A jump queued for the next bar line.
    seek: Option<u32>,
    /// Song beat of the next beat the band plays.
    next_beat: u32,
    /// The position jumped since the last beat, so a held chord must be looked up.
    jumped: bool,
}

impl Default for SongPlayer {
    fn default() -> Self {
        Self {
            slots: [Slot::Hold; MAX_SONG_BEATS],
            marks: [None; MAX_SONG_BARS],
            bars: 0,
            loop_bars: None,
            active: false,
            start_bar: 0,
            seek: None,
            next_beat: 0,
            jumped: true,
        }
    }
}

impl SongPlayer {
    /// Whether the chart, rather than the player, drives the harmony.
    pub fn active(&self) -> bool {
        self.active && self.bars > 0
    }

    pub fn set_active(&mut self, on: bool) {
        self.active = on;
    }

    /// Empties the chart and sets its length; marks and slots start empty.
    pub fn clear(&mut self, bars: u32) {
        self.bars = bars.min(MAX_SONG_BARS as u32);
        self.slots.fill(Slot::Hold);
        self.marks.fill(None);
        self.loop_bars = self.loop_bars.filter(|(_, end)| *end <= self.bars);
    }

    pub fn set_slot(&mut self, beat: u32, slot: Slot) {
        if let Some(s) = self.slots.get_mut(beat as usize) {
            *s = slot;
        }
    }

    pub fn set_mark(&mut self, bar: u32, mark: Option<Request>) {
        if let Some(m) = self.marks.get_mut(bar as usize) {
            *m = mark;
        }
    }

    /// Repeats bars `start..end`; an empty range turns the loop off.
    pub fn set_loop(&mut self, start: u32, end: u32) {
        self.loop_bars = (start < end && end <= MAX_SONG_BARS as u32).then_some((start, end));
    }

    /// Stopped: where the next performance starts. Playing: a jump on the next bar line.
    pub fn seek(&mut self, bar: u32, playing: bool) {
        let bar = bar.min(MAX_SONG_BARS as u32 - 1);
        if playing {
            self.seek = Some(bar);
        } else {
            self.start_bar = bar;
        }
    }

    /// Prepares a performance. Returns whether it opens with the intro and the
    /// variation in force at the start bar, if the chart sets one.
    pub fn start(&mut self) -> (bool, Option<u8>) {
        let start = self.start_bar.min(self.bars.saturating_sub(1));
        self.next_beat = start * BEATS;
        self.seek = None;
        self.jumped = true;
        let intro = self.mark(start) == Some(Request::Intro);
        let variation = (0..=start).rev().find_map(|bar| match self.mark(bar) {
            Some(Request::Variation(v)) => Some(v),
            _ => None,
        });
        (intro, variation)
    }

    /// Advances one beat; `beat_in_bar` is the band's beat, 0 on downbeats.
    pub fn on_beat(&mut self, beat_in_bar: u8) -> SongBeat {
        if beat_in_bar == 0
            && let Some(bar) = self.seek.take()
        {
            self.next_beat = bar * BEATS;
            self.jumped = true;
        }
        let beat = self.next_beat;
        let bar = beat / BEATS;
        let chord = if std::mem::take(&mut self.jumped) { Some(self.chord_at(beat)) } else { self.slot(beat).chord() };

        // Requests go out on the second beat: a variation change then fills
        // the second half of the bar, like the conductor's own changes, and
        // everything else lands on the next bar line.
        let request = if beat_in_bar == 1 {
            match self.following(bar) {
                Some(next) if self.mark(next).is_some_and(|m| m != Request::Fill) => self.mark(next),
                Some(_) => self.mark(bar).filter(|m| *m == Request::Fill),
                None => Some(Request::Ending),
            }
        } else {
            None
        };

        self.next_beat = beat + 1;
        if let Some((start, end)) = self.loop_bars
            && self.next_beat == end * BEATS
        {
            self.next_beat = start * BEATS;
            self.jumped = true;
        }
        SongBeat { beat, bar: (beat_in_bar == 0).then_some(bar), chord, request }
    }

    /// The song beat the next [`SongPlayer::on_beat`] call will play, taking a
    /// queued jump into account.
    pub fn upcoming(&self, beat_in_bar: u8) -> u32 {
        match self.seek {
            Some(bar) if beat_in_bar == 0 => bar * BEATS,
            _ => self.next_beat,
        }
    }

    /// The bar after `bar`, following the loop; `None` past the last bar.
    fn following(&self, bar: u32) -> Option<u32> {
        match self.loop_bars {
            Some((start, end)) if bar + 1 == end => Some(start),
            _ => (bar + 1 < self.bars).then_some(bar + 1),
        }
    }

    fn slot(&self, beat: u32) -> Slot {
        if beat < self.bars * BEATS { self.slots.get(beat as usize).copied().unwrap_or_default() } else { Slot::Hold }
    }

    fn mark(&self, bar: u32) -> Option<Request> {
        if bar < self.bars { self.marks.get(bar as usize).copied().flatten() } else { None }
    }

    /// The harmony in force on `beat`: its own slot or the last one before it.
    fn chord_at(&self, beat: u32) -> Option<Chord> {
        let last = beat.min((self.bars * BEATS).saturating_sub(1));
        (0..=last).rev().find_map(|b| self.slot(b).chord()).flatten()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chord::Quality;

    const C: Chord = Chord::new(0, Quality::Major);
    const F: Chord = Chord::new(5, Quality::Major);
    const G: Chord = Chord::new(7, Quality::Major);

    /// C | F G | C, with B from bar 1.
    fn song() -> SongPlayer {
        let mut s = SongPlayer::default();
        s.clear(3);
        s.set_active(true);
        s.set_slot(0, Slot::Chord(C));
        s.set_slot(4, Slot::Chord(F));
        s.set_slot(6, Slot::Chord(G));
        s.set_slot(8, Slot::Chord(C));
        s.set_mark(1, Some(Request::Variation(1)));
        s
    }

    fn beats(s: &mut SongPlayer, n: u32) -> Vec<SongBeat> {
        (0..n).map(|i| s.on_beat((i % BEATS) as u8)).collect()
    }

    #[test]
    fn chords_change_on_their_beats() {
        let mut s = song();
        s.start();
        let chords: Vec<_> = beats(&mut s, 12).iter().map(|b| b.chord).collect();
        assert_eq!(chords[0], Some(Some(C)));
        assert_eq!(chords[1], None);
        assert_eq!(chords[4], Some(Some(F)));
        assert_eq!(chords[6], Some(Some(G)));
        assert_eq!(chords[8], Some(Some(C)));
    }

    #[test]
    fn marks_are_requested_in_the_bar_before() {
        let mut s = song();
        s.start();
        let b = beats(&mut s, 12);
        assert_eq!(b[1].request, Some(Request::Variation(1)));
        assert_eq!(b[5].request, None);
        assert_eq!(b[9].request, Some(Request::Ending), "the last bar leads into the ending");
        assert_eq!(b[4].bar, Some(1));
    }

    #[test]
    fn loops_wrap_and_restore_the_held_chord() {
        let mut s = song();
        s.set_slot(4, Slot::Hold); // bar 1 holds C from bar 0
        s.set_loop(1, 3);
        s.start();
        let b = beats(&mut s, 20);
        assert!(b.iter().all(|b| b.request != Some(Request::Ending)), "a loop never ends");
        assert_eq!(b[9].request, Some(Request::Variation(1)), "the loop start's mark comes again");
        assert_eq!(b[12].bar, Some(1));
        assert_eq!(b[12].chord, Some(Some(C)), "the chord before the loop start is looked up");
    }

    #[test]
    fn starts_from_a_bar_with_its_chord_and_variation() {
        let mut s = song();
        s.set_mark(0, Some(Request::Intro));
        s.seek(2, false);
        assert_eq!(s.start(), (false, Some(1)));
        let first = s.on_beat(0);
        assert_eq!((first.bar, first.chord), (Some(2), Some(Some(C))));

        s.seek(0, false);
        assert_eq!(s.start(), (true, None));
    }

    #[test]
    fn seeking_while_playing_jumps_on_the_bar_line() {
        let mut s = song();
        s.start();
        beats(&mut s, 2);
        s.seek(2, true);
        assert_eq!(s.on_beat(2).bar, None);
        assert_eq!(s.on_beat(3).bar, None);
        assert_eq!(s.upcoming(0), 8);
        let next = s.on_beat(0);
        assert_eq!((next.bar, next.chord), (Some(2), Some(Some(C))));
        assert_eq!((next.beat, s.upcoming(1)), (8, 9));
    }

    #[test]
    fn a_fill_mark_fills_its_own_bar_unless_the_next_one_changes() {
        let mut s = song();
        s.set_mark(0, Some(Request::Fill));
        s.set_mark(1, None);
        s.start();
        assert_eq!(beats(&mut s, 2)[1].request, Some(Request::Fill));
    }

    #[test]
    fn empty_chart_is_inactive() {
        let mut s = SongPlayer::default();
        s.set_active(true);
        assert!(!s.active());
        s.clear(1);
        assert!(s.active());
    }
}
