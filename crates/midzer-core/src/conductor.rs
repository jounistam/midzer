//! The conductor decides what the band plays next: intro, a main variation,
//! a fill, a rest or the ending. It also turns the player's touch into energy.
//!
//! Decisions happen on beats. Section changes land on the next bar line,
//! except fills, which start on the next beat and run to the end of the bar.

/// What the band is playing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Section {
    Intro,
    Main,
    Fill,
    Ending,
    /// The player stopped; the band holds the chord and waits.
    Rest,
}

impl Section {
    pub const ALL: [Section; 5] = [Section::Intro, Section::Main, Section::Fill, Section::Ending, Section::Rest];
}

/// A section the player asks for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Request {
    Intro,
    Variation(u8),
    Fill,
    Ending,
}

/// What the band does when the player stops.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IdleMode {
    KeepPlaying,
    /// Fill, then hold the chord until the player comes back.
    Rest,
    /// Play the ending and stop.
    End,
}

/// Bars without a note before the band reacts to silence.
pub const IDLE_BARS: u32 = 2;
const BEATS_PER_BAR: u32 = crate::style::BEATS_PER_BAR;

/// Energy thresholds between variations A|B and B|C, with hysteresis.
const VARIATION_THRESHOLDS: [f32; 2] = [0.45, 0.72];
const HYSTERESIS: f32 = 0.04;
/// Bars a variation plays before following may change it again.
const MIN_BARS_PER_VARIATION: u32 = 2;

/// What [`Conductor::on_beat`] asks the band to do.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Cue {
    /// The ending finished: stop now.
    pub stop: bool,
    /// Mark the start of a new section with a cymbal.
    pub crash: bool,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Conductor {
    section: Section,
    variation: u8,
    /// The variation that plays after the current intro, fill or rest.
    next_variation: u8,
    /// Where a fill leads: `Main` or `Rest`.
    after_fill: Section,
    follow: bool,
    energy: f32,
    manual_energy: f32,
    idle_mode: IdleMode,
    pending_fill: bool,
    pending_intro: bool,
    pending_ending: bool,
    beat_notes: u32,
    beat_velocity: u32,
    /// Whether the player has played since the band started; silence before
    /// the first note is not "stopping".
    heard_player: bool,
    idle_beats: u32,
    bars_in_variation: u32,
    /// False until the first downbeat, which starts the first bar rather
    /// than ending one.
    started: bool,
    /// A song is playing: the band never rests or ends because the player
    /// went quiet.
    song: bool,
}

impl Default for Conductor {
    fn default() -> Self {
        Self {
            section: Section::Main,
            variation: 0,
            next_variation: 0,
            after_fill: Section::Main,
            follow: true,
            energy: 0.4,
            manual_energy: 0.4,
            idle_mode: IdleMode::Rest,
            pending_fill: false,
            pending_intro: false,
            pending_ending: false,
            beat_notes: 0,
            beat_velocity: 0,
            heard_player: false,
            idle_beats: 0,
            bars_in_variation: 0,
            started: false,
            song: false,
        }
    }
}

impl Conductor {
    pub fn section(&self) -> Section {
        self.section
    }

    pub fn variation(&self) -> u8 {
        self.variation
    }

    pub fn next_variation(&self) -> u8 {
        self.next_variation
    }

    pub fn energy(&self) -> f32 {
        self.energy
    }

    pub fn follows(&self) -> bool {
        self.follow
    }

    /// A fill is queued for the next beat.
    pub fn fill_pending(&self) -> bool {
        self.pending_fill
    }

    pub fn intro_pending(&self) -> bool {
        self.pending_intro
    }

    pub fn ending_pending(&self) -> bool {
        self.pending_ending
    }

    /// Note velocities are scaled by this, so energy is heard in every part.
    pub fn velocity_scale(&self) -> f32 {
        0.8 + 0.4 * self.energy
    }

    /// Resets for a new performance, optionally starting with the intro.
    pub fn start(&mut self, intro: bool) {
        self.section = if intro { Section::Intro } else { Section::Main };
        self.variation = self.next_variation;
        self.after_fill = Section::Main;
        self.pending_fill = false;
        self.pending_intro = false;
        self.pending_ending = false;
        self.beat_notes = 0;
        self.beat_velocity = 0;
        self.heard_player = false;
        self.idle_beats = 0;
        self.bars_in_variation = 0;
        self.started = false;
    }

    /// Handles a request. `playing` tells whether the band is running.
    pub fn request(&mut self, request: Request, playing: bool) {
        if self.section == Section::Ending && playing {
            return;
        }
        match request {
            Request::Variation(v) => {
                let v = v.min(crate::style::VARIATIONS as u8 - 1);
                self.follow = false;
                self.next_variation = v;
                if !playing {
                    self.variation = v;
                } else if self.section == Section::Main && v != self.variation {
                    self.pending_fill = true;
                }
            }
            Request::Fill if playing => self.pending_fill = true,
            Request::Ending if playing => self.pending_ending = true,
            Request::Intro if playing => self.pending_intro = true,
            _ => {}
        }
    }

    pub fn set_follow(&mut self, follow: bool) {
        self.follow = follow;
        if !follow {
            self.energy = self.manual_energy;
        }
    }

    /// Sets the energy used while not following the player.
    pub fn set_energy(&mut self, energy: f32) {
        self.manual_energy = energy.clamp(0.0, 1.0);
        if !self.follow {
            self.energy = self.manual_energy;
        }
    }

    pub fn set_idle_mode(&mut self, mode: IdleMode) {
        self.idle_mode = mode;
    }

    pub fn set_song(&mut self, on: bool) {
        self.song = on;
    }

    pub fn note_played(&mut self, velocity: u8) {
        self.beat_notes += 1;
        self.beat_velocity += velocity as u32;
        self.heard_player = true;
    }

    /// Advances to `beat` (0 is the downbeat). `keys_held` tells whether the
    /// player is holding any key, which counts as playing.
    pub fn on_beat(&mut self, beat: u8, keys_held: bool) -> Cue {
        let active = self.beat_notes > 0 || keys_held || self.song;
        self.update_energy(keys_held);
        if active || !self.heard_player {
            self.idle_beats = 0;
        } else {
            self.idle_beats += 1;
        }
        let idle = self.idle_beats >= IDLE_BARS * BEATS_PER_BAR;

        let mut cue = Cue::default();
        if beat == 0 && std::mem::replace(&mut self.started, true) {
            if self.section == Section::Ending {
                cue.stop = true;
                return cue;
            }
            self.bars_in_variation += 1;
            if std::mem::take(&mut self.pending_ending) {
                self.pending_fill = false;
                self.section = Section::Ending;
            } else if std::mem::take(&mut self.pending_intro) {
                self.section = Section::Intro;
            } else {
                match self.section {
                    Section::Intro | Section::Fill => {
                        self.section = if self.after_fill == Section::Rest && !active { Section::Rest } else { Section::Main };
                        self.after_fill = Section::Main;
                        cue.crash = self.section == Section::Main;
                        self.enter_variation();
                    }
                    Section::Main if idle && self.idle_mode == IdleMode::End => self.section = Section::Ending,
                    Section::Main if idle && self.idle_mode == IdleMode::Rest => self.section = Section::Rest,
                    _ => {}
                }
            }
        }

        // The band comes back in as soon as the player does.
        if self.section == Section::Rest && active {
            self.section = Section::Main;
            cue.crash = true;
            self.enter_variation();
        }

        if self.section == Section::Main {
            if std::mem::take(&mut self.pending_fill) {
                self.section = Section::Fill;
            } else if beat == 2 {
                // Halfway through a bar: decide whether the next bar changes,
                // and if so lead into it with a fill.
                let target = self.target_variation();
                if self.idle_mode == IdleMode::Rest && self.idle_beats + 2 >= IDLE_BARS * BEATS_PER_BAR {
                    self.section = Section::Fill;
                    self.after_fill = Section::Rest;
                } else if self.follow && target != self.variation && self.bars_in_variation >= MIN_BARS_PER_VARIATION {
                    self.next_variation = target;
                    self.section = Section::Fill;
                }
            }
        }
        cue
    }

    fn enter_variation(&mut self) {
        if self.variation != self.next_variation {
            self.bars_in_variation = 0;
        }
        self.variation = self.next_variation;
    }

    /// Energy follows the player's velocity and note density, smoothed over a
    /// few beats. Held keys keep it where it is; silence lets it sink.
    fn update_energy(&mut self, keys_held: bool) {
        let (notes, velocity) = (self.beat_notes, self.beat_velocity);
        self.beat_notes = 0;
        self.beat_velocity = 0;
        if !self.follow {
            self.energy = self.manual_energy;
            return;
        }
        if notes > 0 {
            let touch = velocity as f32 / notes as f32 / 127.0;
            let density = notes.min(6) as f32 / 6.0;
            let target = 0.7 * touch + 0.3 * density;
            self.energy += (target - self.energy) * 0.35;
        } else if !keys_held && self.heard_player {
            self.energy -= self.energy * 0.08;
        }
    }

    fn target_variation(&self) -> u8 {
        let mut v = self.variation as usize;
        while v < 2 && self.energy > VARIATION_THRESHOLDS[v] + HYSTERESIS {
            v += 1;
        }
        while v > 0 && self.energy < VARIATION_THRESHOLDS[v - 1] - HYSTERESIS {
            v -= 1;
        }
        v as u8
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Runs `bars` bars, calling `play` before each beat; returns the
    /// section at every beat.
    fn run(c: &mut Conductor, bars: u32, mut play: impl FnMut(&mut Conductor, u32)) -> Vec<(Section, u8)> {
        let mut out = Vec::new();
        for i in 0..bars * 4 {
            play(c, i);
            let cue = c.on_beat((i % 4) as u8, false);
            if cue.stop {
                break;
            }
            out.push((c.section(), c.variation()));
        }
        out
    }

    fn chord_each_bar(velocity: u8) -> impl FnMut(&mut Conductor, u32) {
        move |c, beat| {
            if beat % 4 == 0 {
                for _ in 0..3 {
                    c.note_played(velocity);
                }
            }
        }
    }

    #[test]
    fn variation_request_fills_then_switches_on_the_bar() {
        let mut c = Conductor::default();
        c.start(false);
        c.on_beat(0, true);
        c.request(Request::Variation(2), true);
        assert!(!c.follows());
        assert_eq!(c.on_beat(1, true), Cue::default());
        assert_eq!(c.section(), Section::Fill);
        assert_eq!(c.variation(), 0);
        c.on_beat(2, true);
        c.on_beat(3, true);
        let cue = c.on_beat(0, true);
        assert!(cue.crash);
        assert_eq!((c.section(), c.variation()), (Section::Main, 2));
    }

    #[test]
    fn intro_then_main() {
        let mut c = Conductor::default();
        c.start(true);
        let beats = run(&mut c, 2, chord_each_bar(80));
        assert_eq!(beats[0].0, Section::Intro);
        assert_eq!(beats[3].0, Section::Intro);
        assert_eq!(beats[4].0, Section::Main);
    }

    #[test]
    fn ending_plays_one_bar_then_stops() {
        let mut c = Conductor::default();
        c.start(false);
        c.on_beat(0, true);
        c.request(Request::Ending, true);
        for beat in 1..4 {
            assert!(!c.on_beat(beat, true).stop);
            assert_eq!(c.section(), Section::Main);
        }
        assert!(!c.on_beat(0, true).stop);
        assert_eq!(c.section(), Section::Ending);
        for beat in 1..4 {
            c.on_beat(beat, false);
        }
        assert!(c.on_beat(0, false).stop);
    }

    #[test]
    fn hard_playing_raises_the_variation_and_soft_lowers_it() {
        let mut c = Conductor::default();
        c.start(false);
        let busy = |c: &mut Conductor, _| {
            for _ in 0..4 {
                c.note_played(120);
            }
        };
        let beats = run(&mut c, 12, busy);
        assert_eq!(beats.last().unwrap().1, 2, "{beats:?}");
        assert!(beats.iter().any(|b| b.0 == Section::Fill), "changes go through a fill");
        assert!(c.energy() > 0.8);

        let beats = run(&mut c, 12, chord_each_bar(40));
        assert_eq!(beats.last().unwrap().1, 0, "{beats:?}");
    }

    #[test]
    fn following_waits_between_changes() {
        let mut c = Conductor::default();
        c.start(false);
        let beats = run(&mut c, 16, |c, _| {
            for _ in 0..4 {
                c.note_played(127);
            }
        });
        let mut last_change = None;
        for (i, w) in beats.windows(2).enumerate() {
            if w[0].1 != w[1].1 {
                if let Some(prev) = last_change {
                    assert!(i - prev >= 8, "variation changed after {} beats", i - prev);
                }
                last_change = Some(i);
            }
        }
    }

    #[test]
    fn silence_rests_after_a_fill_and_playing_brings_the_band_back() {
        let mut c = Conductor::default();
        c.start(false);
        run(&mut c, 1, chord_each_bar(80));
        let beats = run(&mut c, 4, |_, _| {});
        assert!(beats.iter().any(|b| b.0 == Section::Fill));
        assert_eq!(beats.last().unwrap().0, Section::Rest);

        c.note_played(90);
        let cue = c.on_beat(1, false);
        assert!(cue.crash);
        assert_eq!(c.section(), Section::Main);
    }

    #[test]
    fn silence_before_the_first_note_keeps_the_groove() {
        let mut c = Conductor::default();
        c.start(false);
        let beats = run(&mut c, 6, |_, _| {});
        assert!(beats.iter().all(|b| b.0 == Section::Main));
    }

    #[test]
    fn held_keys_count_as_playing() {
        let mut c = Conductor::default();
        c.start(false);
        c.note_played(80);
        for i in 0..32 {
            c.on_beat(i % 4, true);
            assert_eq!(c.section(), Section::Main);
        }
    }

    #[test]
    fn idle_end_mode_plays_the_ending() {
        let mut c = Conductor::default();
        c.set_idle_mode(IdleMode::End);
        c.start(false);
        run(&mut c, 1, chord_each_bar(80));
        let beats = run(&mut c, 5, |_, _| {});
        assert_eq!(beats.last().unwrap().0, Section::Ending);
        assert!(c.on_beat(0, false).stop);
    }

    #[test]
    fn songs_keep_playing_through_silence() {
        let mut c = Conductor::default();
        c.set_song(true);
        c.start(false);
        run(&mut c, 1, chord_each_bar(80));
        let beats = run(&mut c, 6, |_, _| {});
        assert!(beats.iter().all(|b| b.0 == Section::Main));
    }

    #[test]
    fn manual_energy_overrides_following() {
        let mut c = Conductor::default();
        c.set_energy(0.9);
        assert!((c.energy() - 0.4).abs() < 1e-6, "ignored while following");
        c.set_follow(false);
        assert_eq!(c.energy(), 0.9);
        c.note_played(10);
        c.on_beat(0, true);
        assert_eq!(c.energy(), 0.9);
    }
}
