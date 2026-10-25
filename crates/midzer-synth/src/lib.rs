//! SoundFont playback of engine events, built on rustysynth.

use std::io::Cursor;
use std::sync::Arc;

use midzer_core::{DRUM_CHANNEL, EngineEvent, MIC_CHANNEL, PLAYER_DRUM_CHANNEL, Timed};
use rustysynth::{SoundFont, Synthesizer, SynthesizerSettings};

/// rustysynth applies events at its internal block boundaries, so this bounds
/// event timing jitter (32 frames ≈ 0.7 ms at 48 kHz).
const INTERNAL_BLOCK: usize = 32;
const POLYPHONY: usize = 96;
const DEFAULT_GAIN: f32 = 0.6;

pub struct Synth {
    sample_rate: i32,
    gain: f32,
    /// Whether the player's drum hits sound; an e-kit's module usually plays them.
    kit_sound: bool,
    /// Each channel's last program, kept across a SoundFont load.
    programs: [u8; 16],
    inner: Option<Synthesizer>,
}

impl Synth {
    pub fn new(sample_rate: f32) -> Self {
        Self { sample_rate: sample_rate as i32, gain: DEFAULT_GAIN, kit_sound: false, programs: [0; 16], inner: None }
    }

    pub fn is_loaded(&self) -> bool {
        self.inner.is_some()
    }

    /// Parses an SF2 file. This allocates heavily, so call it before playback
    /// starts, never from inside a render callback that must stay glitch-free.
    pub fn load_soundfont(&mut self, bytes: &[u8]) -> Result<(), String> {
        let font = SoundFont::new(&mut Cursor::new(bytes)).map_err(|e| format!("{e:?}"))?;
        let mut settings = SynthesizerSettings::new(self.sample_rate);
        settings.block_size = INTERNAL_BLOCK;
        settings.maximum_polyphony = POLYPHONY;
        let mut synth = Synthesizer::new(&Arc::new(font), &settings).map_err(|e| format!("{e:?}"))?;
        synth.set_master_volume(self.gain);
        for (channel, &program) in self.programs.iter().enumerate() {
            if program != 0 {
                synth.process_midi_message(channel as i32, 0xC0, program as i32, 0);
            }
        }
        self.inner = Some(synth);
        Ok(())
    }

    pub fn set_gain(&mut self, gain: f32) {
        self.gain = gain.clamp(0.0, 1.0);
        if let Some(synth) = &mut self.inner {
            synth.set_master_volume(self.gain);
        }
    }

    pub fn set_kit_sound(&mut self, on: bool) {
        self.kit_sound = on;
    }

    /// Renders one block, applying each event at its frame. Silent until a
    /// SoundFont is loaded.
    pub fn render(&mut self, events: &[Timed], left: &mut [f32], right: &mut [f32]) {
        for timed in events {
            if let EngineEvent::ProgramChange { channel, program } = timed.event {
                self.programs[channel as usize & 15] = program;
            }
        }
        let Some(synth) = &mut self.inner else {
            left.fill(0.0);
            right.fill(0.0);
            return;
        };
        let mut pos = 0;
        for timed in events {
            let frame = (timed.frame as usize).min(left.len());
            if frame > pos {
                synth.render(&mut left[pos..frame], &mut right[pos..frame]);
                pos = frame;
            }
            apply(synth, &timed.event, self.kit_sound);
        }
        if pos < left.len() {
            synth.render(&mut left[pos..], &mut right[pos..]);
        }
    }
}

fn apply(synth: &mut Synthesizer, event: &EngineEvent, kit_sound: bool) {
    match *event {
        // The microphone's notes are already sounding in the room.
        EngineEvent::NoteOn { channel: MIC_CHANNEL, .. } | EngineEvent::NoteOff { channel: MIC_CHANNEL, .. } => {}
        // Only the drum channel has drum sounds, so the player's hits share the band's kit.
        EngineEvent::NoteOn { channel: PLAYER_DRUM_CHANNEL, key, velocity } => {
            if kit_sound {
                synth.note_on(DRUM_CHANNEL as i32, key as i32, velocity as i32);
            }
        }
        EngineEvent::NoteOff { channel: PLAYER_DRUM_CHANNEL, key } => synth.note_off(DRUM_CHANNEL as i32, key as i32),
        EngineEvent::NoteOn { channel, key, velocity } => synth.note_on(channel as i32, key as i32, velocity as i32),
        EngineEvent::NoteOff { channel, key } => synth.note_off(channel as i32, key as i32),
        EngineEvent::ProgramChange { channel, program } => {
            synth.process_midi_message(channel as i32, 0xC0, program as i32, 0);
        }
        EngineEvent::Transport { playing: false } => synth.note_off_all(false),
        EngineEvent::ChordChanged(_)
        | EngineEvent::Beat { .. }
        | EngineEvent::Transport { .. }
        | EngineEvent::Band(_)
        | EngineEvent::SongPosition { .. }
        | EngineEvent::Hold { .. }
        | EngineEvent::Input { .. }
        | EngineEvent::Tempo { .. }
        | EngineEvent::Timing { .. }
        | EngineEvent::CountIn { .. }
        | EngineEvent::Judged { .. }
            | EngineEvent::Live { .. }
            | EngineEvent::KeysJudged { .. }
            | EngineEvent::LiveForgot
            | EngineEvent::SoloFeel { .. } => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    const SR: f32 = 48_000.0;
    const SOUNDFONT: &str = "../../apps/web/public/soundfonts/GeneralUser-GS.sf2";

    fn loaded() -> Option<Synth> {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(SOUNDFONT);
        let Ok(bytes) = std::fs::read(&path) else {
            eprintln!("skipping: run `node scripts/fetch-soundfont.mjs` first");
            return None;
        };
        let mut synth = Synth::new(SR);
        synth.load_soundfont(&bytes).unwrap();
        Some(synth)
    }

    fn peak(buf: &[f32]) -> f32 {
        buf.iter().fold(0.0, |m, s| m.max(s.abs()))
    }

    #[test]
    fn silent_without_soundfont() {
        let mut synth = Synth::new(SR);
        let (mut l, mut r) = ([1.0; 128], [1.0; 128]);
        synth.render(&[Timed { frame: 0, event: EngineEvent::NoteOn { channel: 0, key: 60, velocity: 100 } }], &mut l, &mut r);
        assert_eq!(peak(&l), 0.0);
    }

    #[test]
    fn rejects_garbage() {
        assert!(Synth::new(SR).load_soundfont(b"not a soundfont").is_err());
    }

    #[test]
    fn plays_piano_and_drums() {
        let Some(mut synth) = loaded() else { return };
        for (channel, key) in [(0, 60), (9, 38)] {
            let (mut l, mut r) = (vec![0.0; 128], vec![0.0; 128]);
            let on = [Timed { frame: 0, event: EngineEvent::NoteOn { channel, key, velocity: 110 } }];
            synth.render(&on, &mut l, &mut r);
            let mut loudest = 0.0f32;
            for _ in 0..40 {
                synth.render(&[], &mut l, &mut r);
                loudest = loudest.max(peak(&l));
            }
            assert!(loudest > 0.01, "channel {channel} silent");
            assert!(loudest <= 1.0, "channel {channel} clipping: {loudest}");
            synth.render(&[Timed { frame: 0, event: EngineEvent::NoteOff { channel, key } }], &mut l, &mut r);
        }
    }

    #[test]
    fn programs_chosen_before_loading_still_apply() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(SOUNDFONT);
        let Ok(bytes) = std::fs::read(&path) else { return };
        // A slow pad swells; the piano is loudest right away.
        let early = |program: Option<u8>| {
            let mut synth = Synth::new(SR);
            let (mut l, mut r) = (vec![0.0; 128], vec![0.0; 128]);
            if let Some(program) = program {
                synth.render(&[Timed { frame: 0, event: EngineEvent::ProgramChange { channel: 0, program } }], &mut l, &mut r);
            }
            synth.load_soundfont(&bytes).unwrap();
            synth.render(&[Timed { frame: 0, event: EngineEvent::NoteOn { channel: 0, key: 60, velocity: 110 } }], &mut l, &mut r);
            (0..8).fold(0.0f32, |m, _| {
                synth.render(&[], &mut l, &mut r);
                m.max(peak(&l))
            })
        };
        assert!(early(Some(89)) < early(None) / 2.0);
    }

    #[test]
    fn player_drums_sound_only_with_kit_sound_on() {
        let Some(mut synth) = loaded() else { return };
        let loudest = |synth: &mut Synth| {
            let (mut l, mut r) = (vec![0.0; 128], vec![0.0; 128]);
            let hit = [Timed { frame: 0, event: EngineEvent::NoteOn { channel: PLAYER_DRUM_CHANNEL, key: 38, velocity: 110 } }];
            synth.render(&hit, &mut l, &mut r);
            (0..40).fold(0.0f32, |m, _| {
                synth.render(&[], &mut l, &mut r);
                m.max(peak(&l))
            })
        };
        assert_eq!(loudest(&mut synth), 0.0);
        synth.set_kit_sound(true);
        assert!(loudest(&mut synth) > 0.01);
    }

    #[test]
    fn events_are_sample_accurate_up_to_one_internal_block() {
        let Some(mut synth) = loaded() else { return };
        let mut out = Vec::new();
        for block in 0..2 {
            let (mut l, mut r) = (vec![0.0; 128], vec![0.0; 128]);
            let on = [Timed { frame: 96, event: EngineEvent::NoteOn { channel: 0, key: 60, velocity: 127 } }];
            synth.render(if block == 0 { &on } else { &[] }, &mut l, &mut r);
            out.extend(l);
        }
        // rustysynth starts a voice on its next internal block: a constant
        // one-block delay, no jitter.
        let first_sound = out.iter().position(|s| s.abs() > 1e-6).unwrap_or(usize::MAX);
        assert_eq!(first_sound, 96 + INTERNAL_BLOCK + 1);
    }
}

