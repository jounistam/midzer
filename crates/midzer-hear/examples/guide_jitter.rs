//! How steady the engine's live beats are with the listener guiding it, on
//! the Playwright test's groove (120 BPM, kick and snare, C then G): prints
//! each beat shown with its error from the true beat, and each guide.
//!   cargo run --release -p midzer-hear --example guide_jitter

use midzer_core::{Command, Engine, EngineEvent};
use midzer_hear::{Heard, Hearing};

const SR: usize = 48_000;

fn groove(seconds: f64) -> Vec<f32> {
    let beat = SR / 2;
    let n = (seconds * SR as f64) as usize;
    let mut seed = 1u32;
    let mut noise = || {
        seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        seed as f32 / 2f32.powi(32) * 2.0 - 1.0
    };
    (0..n)
        .map(|j| {
            let i = j % (beat * 16);
            let eighth = i / (beat / 2);
            let t = (i % (beat / 2)) as f32 / SR as f32;
            let mut x = if eighth.is_multiple_of(2) {
                if (eighth / 2).is_multiple_of(2) { 0.8 * (2.0 * std::f32::consts::PI * 60.0 * t).sin() * (-t * 25.0).exp() } else { 0.5 * noise() * (-t * 60.0).exp() }
            } else {
                0.12 * noise() * (-t * 200.0).exp()
            };
            let chord: [i32; 4] = if i < beat * 8 { [48, 60, 64, 67] } else { [43, 55, 59, 62] };
            for key in chord {
                let hz = 440.0 * 2f32.powf((key - 69) as f32 / 12.0);
                for h in 1..=4 {
                    x += 0.035 / h as f32 * (2.0 * std::f32::consts::PI * hz * h as f32 * j as f32 / SR as f32).sin();
                }
            }
            x.clamp(-1.0, 1.0)
        })
        .collect()
}

fn main() {
    let audio = groove(30.0);
    let mut engine = Engine::new(SR as f32);
    engine.handle(Command::SetLive { on: true });
    let mut hearing = Hearing::with_models(SR as f32).unwrap();
    for (i, block) in audio.chunks(128).enumerate() {
        let now = i * 128;
        hearing.push(block, now as u64);
        if let Some(Heard::Grid(g)) = hearing.step() {
            println!("{:6.2}s guide beat {:.4}s period {:.2} one {:?} conf {:.2}", now as f64 / SR as f64, g.beat / SR as f64, g.period, g.one, g.confidence);
            engine.handle(Command::LiveGuide { frame: g.beat.round() as u32, period: g.period as f32, one: g.one, confidence: (g.confidence * 255.0) as u8 });
        }
        engine.listen_live(block);
        for t in engine.process(block.len() as u32) {
            if let EngineEvent::Beat { beat, .. } = t.event {
                let at = (now + t.frame as usize) as f64 / SR as f64;
                let err = (at - (at * 2.0).round() / 2.0) * 1000.0;
                println!("{at:6.3}s beat {beat} error {err:+.1} ms");
            }
        }
        engine.end_block();
    }
}
