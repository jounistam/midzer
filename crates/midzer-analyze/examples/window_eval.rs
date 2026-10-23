//! How well does the offline beat tracker on only the last W seconds predict
//! the beats that come next? The same test as the neural one in the play-along
//! research: anchors every 7 s, beats extrapolated over the next 4 s.
//!   BT_REF=<dir> cargo run --release -p midzer-analyze --example window_eval -- a.mp3 …

use midzer_analyze::{RATE, beats, decode, resample};

const EDGE: f32 = 0.15;

fn main() {
    let dir = std::env::var("BT_REF").expect("BT_REF");
    let windows = [3.0f32, 4.0, 6.0, 8.0, 12.0];
    let mut tot = [[0.0f32; 4]; 5];
    for path in std::env::args().skip(1) {
        let name = path.rsplit('/').next().unwrap().to_string();
        let text = std::fs::read_to_string(format!("{dir}/{name}.bt")).unwrap();
        let rb: Vec<f32> = text.lines().map(|l| l.split(' ').next().unwrap().parse().unwrap()).collect();
        let mut iv: Vec<f32> = rb.windows(2).map(|w| w[1] - w[0]).collect();
        iv.sort_by(f32::total_cmp);
        let rper = iv[iv.len() / 2];
        let d = decode(std::fs::read(&path).unwrap()).unwrap();
        let mix = resample(&d.mono(), d.sample_rate, RATE);
        let dur = mix.len() as f32 / RATE as f32;
        let fps = beats::frames_per_second();
        for (wi, &w) in windows.iter().enumerate() {
            let mut t = w.max(10.0);
            while t < dur - 5.0 {
                if rb.iter().any(|&b| b > t - w) && rb.iter().any(|&b| b > t + 4.0) {
                    let x = &mix[((t - w) * RATE as f32) as usize..(t * RATE as f32) as usize];
                    let on = beats::onsets(x);
                    let period = beats::tempo_period(&on.all);
                    let b: Vec<f32> = beats::track(&on.all, period).iter().map(|&f| f as f32 / fps + t - w).filter(|&b| b < t - EDGE).collect();
                    tot[wi][0] += 1.0;
                    if b.len() >= 3 {
                        let mut iv: Vec<f32> = b.windows(2).map(|w| w[1] - w[0]).collect();
                        iv.sort_by(f32::total_cmp);
                        let per = iv[iv.len() / 2];
                        let last = b[b.len() - 1];
                        for k in 1..40 {
                            let p = last + k as f32 * per;
                            if p >= t && p < t + 4.0 {
                                tot[wi][2] += 1.0;
                                if rb.iter().any(|&r| (r - p).abs() < 0.07) {
                                    tot[wi][1] += 1.0;
                                }
                            }
                        }
                        if (per / rper - 1.0).abs() < 0.04 {
                            tot[wi][3] += 1.0;
                        }
                    }
                }
                t += 7.0;
            }
        }
        eprintln!("{name}");
    }
    for (wi, w) in windows.iter().enumerate() {
        let [n, h, nb, tok] = tot[wi];
        println!("window {w:4.1}s: next-beats on ref {:5.1}%  tempo ok {:5.1}%  (anchors {n})", 100.0 * h / nb.max(1.0), 100.0 * tok / n);
    }
}
