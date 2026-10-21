//! Runs the full transcription on a file and prints the result as JSON:
//!   cargo run --release -p midzer-stems --example transcribe -- song.mp3 models-dir [stems-dir]

use midzer_stems::{Options, transcribe_file};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let bytes = std::fs::read(&args[0]).expect("read the file");
    let cancel = std::sync::atomic::AtomicBool::new(false);
    let options = Options { models: args[1].as_ref(), stems_out: args.get(2).map(Into::into), cancel: &cancel };
    let start = std::time::Instant::now();
    let mut last = None;
    let analysis = transcribe_file(bytes, &options, &mut |f, step| {
        let tenth = (f * 100.0) as u32;
        if last != Some((step, tenth)) {
            let rss_mb = std::fs::read_to_string("/proc/self/statm").ok().and_then(|s| s.split_whitespace().nth(1)?.parse::<u64>().ok()).map_or(0, |p| p * 4 / 1024);
            eprintln!("{:>7.1}s {:>4.0}% {step:?} {rss_mb} MB", start.elapsed().as_secs_f32(), f * 100.0);
            last = Some((step, tenth));
        }
    })
    .unwrap_or_else(|e| panic!("{e}"));
    println!("{}", serde_json::to_string(&analysis).unwrap());
}
