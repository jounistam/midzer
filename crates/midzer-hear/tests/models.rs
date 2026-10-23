//! The Rust networks and features against PyTorch, torchaudio and librosa on
//! a synthetic groove (`tests/data`, written by `scripts/models/export.py`).

use midzer_hear::btc::{BINS, CLASSES};
use midzer_hear::mel::Mel;
use midzer_hear::mzm::Weights;
use midzer_hear::{BEAT_THIS, BTC, BeatThis, Btc, cqt::Cqt};

fn data(name: &str) -> Weights {
    Weights::parse(&std::fs::read(format!("{}/tests/data/{name}", env!("CARGO_MANIFEST_DIR"))).unwrap()).unwrap()
}

fn tensor(w: &Weights, name: &str) -> Vec<f32> {
    w.get(name).unwrap().data.clone()
}

fn max_diff(a: &[f32], b: &[f32]) -> f32 {
    assert_eq!(a.len(), b.len());
    a.iter().zip(b).map(|(x, y)| (x - y).abs()).fold(0.0, f32::max)
}

#[test]
fn mel_matches_torchaudio() {
    let d = data("beat-this.mzm");
    let model = BeatThis::load(BEAT_THIS).unwrap();
    let mel = Mel::new(model.mel_filters.clone()).whole(&tensor(&d, "audio"));
    let want = tensor(&d, "mel");
    // The filterbank is stored as f16.
    let diff = max_diff(&mel, &want);
    assert!(diff < 0.02, "mel differs by {diff}");
}

#[test]
fn beat_this_matches_pytorch() {
    let d = data("beat-this.mzm");
    let mut model = BeatThis::load(BEAT_THIS).unwrap();
    let mel = tensor(&d, "mel");
    let frames = mel.len() / 128;
    let start = std::time::Instant::now();
    let out = model.run(&mel, frames);
    eprintln!("{frames} frames in {:?}", start.elapsed());
    let (beat, down) = (tensor(&d, "beat"), tensor(&d, "downbeat"));
    let (db, dd) = (max_diff(&out.beat, &beat), max_diff(&out.downbeat, &down));
    eprintln!("beat logits differ by {db}, downbeat by {dd}");
    // f16 weights: small differences, and the same frames above zero.
    assert!(db < 0.3 && dd < 0.3);
    let peaks = |x: &[f32]| x.iter().map(|&v| v > 0.0).collect::<Vec<_>>();
    let agree = peaks(&out.beat).iter().zip(peaks(&beat)).filter(|(a, b)| *a == b).count();
    assert!(agree >= frames - 2, "{agree} of {frames} frames agree");
}

#[test]
fn cqt_matches_librosa() {
    let d = data("btc.mzm");
    let got = Cqt::new().whole(&tensor(&d, "audio"));
    let want = tensor(&d, "cqt");
    assert_eq!(got.len(), want.len());
    let mut diffs: Vec<f32> = got.iter().zip(&want).map(|(a, b)| (a - b).abs()).collect();
    diffs.sort_by(f32::total_cmp);
    let (median, p95) = (diffs[diffs.len() / 2], diffs[diffs.len() * 95 / 100]);
    eprintln!("log-CQT differs by a median of {median}, 95 % within {p95}");
    assert!(median < 0.03 && p95 < 0.3);
}

#[test]
fn btc_matches_pytorch() {
    let d = data("btc.mzm");
    let mut model = Btc::load(BTC).unwrap();
    let cqt = tensor(&d, "cqt");
    let n = cqt.len() / BINS;
    let start = std::time::Instant::now();
    let logits = model.run(&cqt, n);
    eprintln!("{n} frames in {:?}", start.elapsed());
    let want = tensor(&d, "logits");
    let diff = max_diff(&logits, &want);
    eprintln!("logits differ by {diff}");
    assert!(diff < 0.05);
    let best = |x: &[f32]| x.as_chunks::<CLASSES>().0.iter().map(|r| (0..CLASSES).max_by(|&a, &b| r[a].total_cmp(&r[b])).unwrap()).collect::<Vec<_>>();
    assert_eq!(best(&logits), best(&want));
}

