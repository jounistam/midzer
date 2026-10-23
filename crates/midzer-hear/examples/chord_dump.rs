//! Prints the chords the listener hears in a recording, run by run, to check them by ear or against a reference.
//!   cargo run --release -p midzer-hear --example chord_dump -- song.mp3 [seconds]

use midzer_analyze::{decode, resample};
use midzer_hear::{Heard, Hearing};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let seconds: f64 = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(30.0);
    let d = decode(std::fs::read(&args[0]).unwrap()).unwrap();
    let audio = resample(&d.mono(), d.sample_rate, 48_000);
    let mut hearing = Hearing::with_models(48_000.0).unwrap();
    for (i, block) in audio.chunks(2048).enumerate().take((seconds * 48_000.0 / 2048.0) as usize) {
        hearing.push(block, (i * 2048) as u64);
        while let Some(heard) = hearing.step() {
            let now = (i + 1) as f64 * 2048.0 / 48_000.0;
            if let Heard::Chords(spans) = heard {
                let text: Vec<String> = spans.iter().map(|s| format!("{:.1}-{:.1} {}", s.from / 48_000.0, s.to / 48_000.0, s.chord.map_or("N".into(), |c| c.to_string()))).collect();
                println!("{now:5.1}s: {}", text.join(" | "));
            }
        }
    }
}
