//! Song playback: a chord chart the band follows instead of the player's
//! chords, with section marks, a loop and a start position.
//!
//! The chart is loaded through commands (one chord slot per beat, one mark
//! and one length per bar) into fixed arrays, so loading and playing never
//! allocate. It can be reloaded while playing; the band picks up the changes
//! on the next beat.
//!
//! Bars are [`BEATS_PER_BAR`] beats unless the chart says otherwise (1 to
//! [`MAX_BAR_BEATS`]). Song beats count from the start of the song, one after
//! another, whatever the bars.

use crate::chord::Chord;
use crate::conductor::Request;
use crate::style::BEATS_PER_BAR;

pub const MAX_SONG_BARS: usize = 512;
/// The longest bar, in beats.
pub const MAX_BAR_BEATS: u32 = 7;
pub const MAX_SONG_BEATS: usize = MAX_SONG_BARS * MAX_BAR_BEATS as usize;

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
    pub(crate) fn chord(self) -> Option<Option<Chord>> {
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
    /// The beat's place in its bar, 0 on downbeats.
    pub beat_in_bar: u8,
    /// How many beats its bar has.
    pub bar_beats: u8,
    /// The harmony from this beat on; `None` keeps the current one.
    pub chord: Option<Option<Chord>>,
    /// A section change for the conductor, timed to land on the next bar.
    pub request: Option<Request>,
}

pub struct SongPlayer {
    slots: [Slot; MAX_SONG_BEATS],
    marks: [Option<Request>; MAX_SONG_BARS],
    bar_beats: [u8; MAX_SONG_BARS],
    /// The song beat each bar starts on, and the end of the last one.
    bar_start: [u16; MAX_SONG_BARS + 1],
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
            bar_beats: [BEATS_PER_BAR as u8; MAX_SONG_BARS],
            bar_start: std::array::from_fn(|b| (b as u32 * BEATS_PER_BAR) as u16),
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

    /// Empties the chart and sets its length; marks and slots start empty,
    /// and every bar has the usual four beats.
    pub fn clear(&mut self, bars: u32) {
        self.bars = bars.min(MAX_SONG_BARS as u32);
        self.slots.fill(Slot::Hold);
        self.marks.fill(None);
        self.bar_beats.fill(BEATS_PER_BAR as u8);
        self.recount(0);
        self.loop_bars = self.loop_bars.filter(|(_, end)| *end <= self.bars);
    }

    /// Sets how many beats bar `bar` has (1 to [`MAX_BAR_BEATS`]).
    pub fn set_bar_beats(&mut self, bar: u32, beats: u8) {
        let beats = beats.clamp(1, MAX_BAR_BEATS as u8);
        if let Some(b) = self.bar_beats.get_mut(bar as usize)
            && *b != beats
        {
            *b = beats;
            self.recount(bar as usize);
        }
    }

    fn recount(&mut self, from: usize) {
        for b in from..MAX_SONG_BARS {
            self.bar_start[b + 1] = self.bar_start[b] + self.bar_beats[b] as u16;
        }
    }

    /// The song beat bar `bar` starts on; past the last bar, the end of the chart.
    pub fn bar_start(&self, bar: u32) -> u32 {
        self.bar_start[bar.min(self.bars) as usize] as u32
    }

    /// The chart's length in beats.
    pub fn beats(&self) -> u32 {
        self.bar_start(self.bars)
    }

    /// The bar song beat `beat` is in, and its place in it. Beats past the
    /// end count on in bars of the last bar's length.
    pub fn bar_of(&self, beat: u32) -> (u32, u8) {
        let end = self.beats();
        if beat >= end {
            let len = self.bar_beats.get(self.bars.saturating_sub(1) as usize).map_or(BEATS_PER_BAR, |&b| b as u32);
            let past = beat - end;
            return (self.bars + past / len, (past % len) as u8);
        }
        let bar = self.bar_start[..=self.bars as usize].partition_point(|&s| s as u32 <= beat) - 1;
        (bar as u32, (beat - self.bar_start[bar] as u32) as u8)
    }

    fn bar_len(&self, bar: u32) -> u8 {
        if bar < self.bars { self.bar_beats[bar as usize] } else { self.bar_beats[self.bars.saturating_sub(1) as usize] }
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
        self.next_beat = self.bar_start(start);
        self.seek = None;
        self.jumped = true;
        let intro = self.mark(start) == Some(Request::Intro);
        let variation = (0..=start).rev().find_map(|bar| match self.mark(bar) {
            Some(Request::Variation(v)) => Some(v),
            _ => None,
        });
        (intro, variation)
    }

    /// Advances one beat. The song says where its bars start.
    pub fn on_beat(&mut self) -> SongBeat {
        if self.bar_of(self.next_beat).1 == 0
            && let Some(bar) = self.seek.take()
        {
            self.next_beat = self.bar_start(bar);
            self.jumped = true;
        }
        let beat = self.next_beat;
        let (bar, beat_in_bar) = self.bar_of(beat);
        let bar_beats = self.bar_len(bar);
        let chord = if std::mem::take(&mut self.jumped) { Some(self.chord_at(beat)) } else { self.slot(beat).chord() };

        // Requests go out on the second beat (the first of a one-beat bar): a
        // variation change then fills the rest of the bar, like the
        // conductor's own changes, and everything else lands on the next bar line.
        let request = if beat_in_bar == 1.min(bar_beats - 1) {
            match self.following(bar) {
                Some(next) if self.mark(next).is_some_and(|m| m != Request::Fill) => self.mark(next),
                Some(_) => self.mark(bar).filter(|m| *m == Request::Fill),
                None => Some(Request::Ending),
            }
        } else {
            None
        };

        self.next_beat = beat + 1;
        // At the loop's end, or past it after the chart got shorter, the next
        // bar starts the loop again.
        if let Some((start, end)) = self.loop_bars
            && self.bar_of(self.next_beat).1 == 0
            && self.next_beat >= self.bar_start(end)
        {
            self.next_beat = self.bar_start(start);
            self.jumped = true;
        }
        SongBeat { beat, bar: (beat_in_bar == 0).then_some(bar), beat_in_bar, bar_beats, chord, request }
    }

    /// The song beat the next [`SongPlayer::on_beat`] call will play, taking a
    /// queued jump into account.
    pub fn upcoming(&self) -> u32 {
        match self.seek {
            Some(bar) if self.bar_of(self.next_beat).1 == 0 => self.bar_start(bar),
            _ => self.next_beat,
        }
    }

    /// The bar after `bar`, following the loop; `None` past the last bar.
    fn following(&self, bar: u32) -> Option<u32> {
        match self.loop_bars {
            Some((start, end)) if bar + 1 >= end => Some(start),
            _ => (bar + 1 < self.bars).then_some(bar + 1),
        }
    }

    fn slot(&self, beat: u32) -> Slot {
        if beat < self.beats() { self.slots.get(beat as usize).copied().unwrap_or_default() } else { Slot::Hold }
    }

    fn mark(&self, bar: u32) -> Option<Request> {
        if bar < self.bars { self.marks.get(bar as usize).copied().flatten() } else { None }
    }

    /// The harmony in force on `beat`: its own slot or the last one before it.
    pub fn chord_at(&self, beat: u32) -> Option<Chord> {
        let last = beat.min(self.beats().saturating_sub(1));
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
        (0..n).map(|_| s.on_beat()).collect()
    }

    #[test]
    fn a_shorter_chart_loops_back_at_the_next_bar() {
        let mut s = song();
        s.clear(8);
        s.set_loop(0, 8);
        s.start();
        beats(&mut s, 6 * 4 + 1);
        // Reloaded as a two-bar loop in the middle of bar 6.
        s.clear(2);
        s.set_loop(0, 2);
        let rest = beats(&mut s, 7);
        assert!(rest.iter().all(|b| b.request != Some(Request::Ending)), "{rest:?}");
        assert_eq!(rest[3].beat, 0, "back to the start on the bar line");
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
        let first = s.on_beat();
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
        assert_eq!(s.on_beat().bar, None);
        assert_eq!(s.on_beat().bar, None);
        assert_eq!(s.upcoming(), 8);
        let next = s.on_beat();
        assert_eq!((next.bar, next.chord), (Some(2), Some(Some(C))));
        assert_eq!((next.beat, s.upcoming()), (8, 9));
    }

    #[test]
    fn a_fill_mark_fills_its_own_bar_unless_the_next_one_changes() {
        let mut s = song();
        s.set_mark(0, Some(Request::Fill));
        s.set_mark(1, None);
        s.start();
        assert_eq!(beats(&mut s, 2)[1].request, Some(Request::Fill));
    }

    /// Bars of 4, 2, 4 and 3 beats: C | F | G | C.
    fn odd() -> SongPlayer {
        let mut s = SongPlayer::default();
        s.clear(4);
        s.set_active(true);
        s.set_bar_beats(1, 2);
        s.set_bar_beats(3, 3);
        for (beat, chord) in [(0, C), (4, F), (6, G), (10, C)] {
            s.set_slot(beat, Slot::Chord(chord));
        }
        s
    }

    #[test]
    fn bars_of_other_lengths_start_where_the_chart_says() {
        let mut s = odd();
        assert_eq!((s.bar_start(2), s.bar_start(3), s.beats()), (6, 10, 13));
        assert_eq!((s.bar_of(5), s.bar_of(6), s.bar_of(12), s.bar_of(14)), ((1, 1), (2, 0), (3, 2), (4, 1)));
        s.start();
        let b = beats(&mut s, 13);
        let downbeats: Vec<_> = b.iter().filter_map(|b| b.bar.map(|bar| (bar, b.beat))).collect();
        assert_eq!(downbeats, [(0, 0), (1, 4), (2, 6), (3, 10)]);
        assert_eq!((b[5].beat_in_bar, b[5].bar_beats, b[12].beat_in_bar, b[12].bar_beats), (1, 2, 2, 3));
        assert_eq!((b[4].chord, b[6].chord, b[10].chord), (Some(Some(F)), Some(Some(G)), Some(Some(C))));
        assert_eq!(b[11].request, Some(Request::Ending), "asked on the second beat of the last bar");
    }

    #[test]
    fn odd_bars_loop_and_seek_on_their_bar_lines() {
        let mut s = odd();
        s.set_loop(1, 3);
        s.seek(1, false);
        s.start();
        let b: Vec<_> = beats(&mut s, 12).iter().map(|b| b.beat).collect();
        assert_eq!(b, [4, 5, 6, 7, 8, 9, 4, 5, 6, 7, 8, 9]);

        s.set_loop(0, 0);
        assert_eq!(s.on_beat().beat, 4);
        s.seek(3, true);
        assert_eq!(s.upcoming(), 5, "the jump waits for the bar line");
        assert_eq!(s.on_beat().beat, 5);
        assert_eq!(s.upcoming(), 10);
        assert_eq!(s.on_beat().bar, Some(3));
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
