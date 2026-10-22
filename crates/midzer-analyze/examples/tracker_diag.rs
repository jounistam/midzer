//! Prints the live beat tracker's state every second for one recording:
//!   cargo run --release -p midzer-analyze --example tracker_diag -- song.mp3

use midzer_analyze::{decode, resample};
use midzer_core::{BeatList, BeatTracker};

fn main() {
    let path = std::env::args().nth(1).expect("file");
    let seconds: f32 = std::env::args().nth(2).map_or(60.0, |s| s.parse().unwrap());
    let decoded = decode(std::fs::read(&path).unwrap()).unwrap();
    let audio = resample(&decoded.mono(), decoded.sample_rate, 48_000);
    let mut tracker = BeatTracker::new(48_000.0);
    let mut next = 1.0;
    let mut beats = 0;
    for (i, block) in audio.chunks(128).enumerate() {
        let mut out = BeatList::default();
        tracker.push(block, (i * 128) as u64, &mut out);
        beats += out.as_slice().len();
        let t = (i * 128) as f32 / 48_000.0;
        if t >= next {
            next += 1.0;
            println!("{t:5.1}s est {:>6.1} loop {:>6.1} conf {:.2} locked {} beats {beats} level {:.0}", tracker.estimate_bpm().unwrap_or(0.0), tracker.bpm().unwrap_or(0.0), tracker.confidence(), tracker.locked() as u8, tracker.level_db());
            beats = 0;
        }
        if t > seconds {
            break;
        }
    }
}
