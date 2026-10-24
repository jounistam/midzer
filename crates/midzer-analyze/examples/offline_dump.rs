//! Writes what the offline analysis reads from each file, for scoring against a
//! reference with mir_eval: `<out>/<name>.beats` ("time is_downbeat") and
//! `<out>/<name>.lab` (chords per beat, in Harte notation).
//!   cargo run --release -p midzer-analyze --example offline_dump -- <out> a.mp3 …

use midzer_analyze::analyze_file;
use midzer_core::Quality;

const ROOTS: [&str; 12] = ["C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B"];

fn harte(root: u8, quality: Quality) -> String {
    let q = match quality {
        Quality::Major => "maj",
        Quality::Minor => "min",
        Quality::Dominant7 => "7",
        Quality::Minor7 => "min7",
        Quality::Major7 => "maj7",
        Quality::Diminished => "dim",
        Quality::Augmented => "aug",
        Quality::Sus2 => "sus2",
        Quality::Sus4 => "sus4",
        Quality::Power => "5",
        Quality::Major6 => "maj6",
        Quality::Minor6 => "min6",
        Quality::HalfDiminished7 => "hdim7",
        Quality::Diminished7 => "dim7",
        Quality::Add9 => "maj(9)",
    };
    format!("{}:{q}", ROOTS[root as usize % 12])
}

fn main() {
    let mut args = std::env::args().skip(1);
    let out = args.next().expect("usage: offline_dump <out> files…");
    std::fs::create_dir_all(&out).unwrap();
    for path in args {
        let name = path.rsplit('/').next().unwrap();
        let start = std::time::Instant::now();
        let a = analyze_file(std::fs::read(&path).unwrap(), &mut |_, _| {}).unwrap();
        let ones = a.downbeats();
        let beats: String = a.beat_times.iter().zip(&ones).map(|(t, &one)| format!("{t:.3} {}\n", one as u8)).collect();
        std::fs::write(format!("{out}/{name}.beats"), beats).unwrap();
        let mut lab = String::new();
        for (i, c) in a.chords.iter().enumerate() {
            let (from, to) = (a.beat_times[i], a.beat_times[i + 1]);
            let label = c.map_or("N".to_string(), |c| harte(c.root, Quality::ALL[c.quality as usize]));
            lab += &format!("{from:.3} {to:.3} {label}\n");
        }
        std::fs::write(format!("{out}/{name}.lab"), lab).unwrap();
        eprintln!("{name}: {:.1}s", start.elapsed().as_secs_f32());
    }
}
