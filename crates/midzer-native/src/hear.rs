//! The listening thread: runs `midzer-hear` over the live source while it is
//! followed. The audio callback hands it blocks tagged with their engine
//! frames; its beat grids go to the engine as `LiveGuide` commands and its
//! chords to the forwarder, for the UI.

use std::sync::atomic::Ordering;
use std::sync::{Arc, mpsc};
use std::time::Duration;

use midzer_core::{Command, Quality};
use midzer_hear::{Heard, Hearing};

use crate::Shared;

const BLOCK: usize = 128;
/// About 11 s of 48 kHz audio: room for a network run.
pub(crate) const TAP_BLOCKS: usize = 4096;
const POLL: Duration = Duration::from_millis(25);

/// One block of the live source and the engine frame of its first sample.
#[derive(Clone, Copy)]
pub(crate) struct TapBlock {
    frame: u64,
    len: u16,
    samples: [f32; BLOCK],
}

impl TapBlock {
    pub(crate) fn new(frame: u64, input: &[f32]) -> Self {
        let mut samples = [0.0; BLOCK];
        let len = input.len().min(BLOCK);
        samples[..len].copy_from_slice(&input[..len]);
        Self { frame, len: len as u16, samples }
    }
}

pub(crate) fn listen(shared: Arc<Shared>, chords: mpsc::Sender<Vec<f64>>) {
    let mut hearing: Option<Hearing> = None;
    let mut generation = shared.live_generation.load(Ordering::Acquire);
    let mut run: Vec<f32> = Vec::new();
    let mut run_start = 0u64;
    while shared.running.load(Ordering::Acquire) {
        let now = shared.live_generation.load(Ordering::Acquire);
        if now != generation {
            generation = now;
            if let Some(h) = hearing.as_mut() {
                h.reset();
            }
        }
        if !shared.live_open.load(Ordering::Acquire) {
            while shared.tap.pop().is_some() {}
            std::thread::sleep(POLL);
            continue;
        }
        // The networks load once, when first needed.
        let h = match hearing.as_mut() {
            Some(h) => h,
            None => match Hearing::with_models(shared.sample_rate as f32) {
                Ok(h) => hearing.insert(h),
                Err(e) => {
                    eprintln!("midzer: the listener did not load: {e}");
                    return;
                }
            },
        };
        while let Some(block) = shared.tap.pop() {
            if !run.is_empty() && block.frame != run_start + run.len() as u64 {
                h.push(&run, run_start);
                run.clear();
            }
            if run.is_empty() {
                run_start = block.frame;
            }
            run.extend_from_slice(&block.samples[..block.len as usize]);
        }
        if !run.is_empty() {
            h.push(&run, run_start);
            run.clear();
        }
        match h.step() {
            Some(Heard::Grid(g)) => {
                let cmd = Command::LiveGuide { frame: g.beat.round() as u64 as u32, period: g.period as f32, one: g.one, confidence: (g.confidence * 255.0) as u8 };
                let _ = shared.commands.push(cmd.encode());
            }
            Some(Heard::Chords(spans)) => {
                let flat = spans
                    .iter()
                    .flat_map(|s| {
                        let code = s.chord.map_or(-1.0, |c| (c.root as usize * 16 + Quality::ALL.iter().position(|&q| q == c.quality).unwrap_or(0)) as f64);
                        [s.from, s.to, code, s.predicted as u8 as f64]
                    })
                    .collect();
                let _ = chords.send(flat);
            }
            None => std::thread::sleep(POLL),
        }
    }
}
