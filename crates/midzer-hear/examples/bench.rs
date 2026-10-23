//! Times the two networks on windows of the sizes live listening uses.
//!   cargo run --release -p midzer-hear --example bench

use midzer_hear::{BEAT_THIS, BTC, BeatThis, Btc};

fn main() {
    let mut beat = BeatThis::load(BEAT_THIS).unwrap();
    let mut chords = Btc::load(BTC).unwrap();
    for frames in [300, 400, 600] {
        let mel: Vec<f32> = (0..frames * 128).map(|i| ((i as f32 * 0.013).sin() + 1.0) * 2.0).collect();
        beat.run(&mel, frames);
        let start = std::time::Instant::now();
        for _ in 0..3 {
            beat.run(&mel, frames);
        }
        println!("Beat This! small, {frames} frames ({} s): {:.0} ms", frames / 50, start.elapsed().as_secs_f32() * 1000.0 / 3.0);
    }
    let cqt: Vec<f32> = (0..108 * 144).map(|i| (i as f32 * 0.07).sin() - 3.0).collect();
    chords.run(&cqt, 108);
    let start = std::time::Instant::now();
    for _ in 0..5 {
        chords.run(&cqt, 108);
    }
    println!("BTC, 108 frames: {:.0} ms", start.elapsed().as_secs_f32() * 1000.0 / 5.0);
}
