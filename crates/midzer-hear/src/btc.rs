//! BTC, the bi-directional transformer for chord recognition (Park, Choi,
//! Chung and Kim, ISMIR 2019), with its large vocabulary: 12 roots × 14
//! qualities, "X" (some other chord) and "N" (no chord). It reads 108 frames
//! of log-CQT (10 s, see [`crate::cqt`]) and names a chord per frame. Each
//! layer runs one transformer block that only looks back and one that only
//! looks ahead, and merges them.

use midzer_core::{Chord, Quality};

use crate::mzm::Weights;
use crate::nn::{AttendScratch, Linear, Mask, attend, conv_matrix, layer_norm, matmul, relu};

pub const FRAMES: usize = 108;
pub const BINS: usize = 144;
pub const CLASSES: usize = 170;
const DIM: usize = 128;
const HEADS: usize = 4;
const HEAD: usize = DIM / HEADS;
const LAYERS: usize = 8;

/// The qualities in BTC's order.
const QUALITIES: [Quality; 14] = [
    Quality::Minor,
    Quality::Major,
    Quality::Diminished,
    Quality::Augmented,
    Quality::Minor6,
    Quality::Major6,
    Quality::Minor7,
    // Minor-major seventh: the nearest the engine has.
    Quality::Minor,
    Quality::Major7,
    Quality::Dominant7,
    Quality::Diminished7,
    Quality::HalfDiminished7,
    Quality::Sus2,
    Quality::Sus4,
];

/// The chord of a class, or `None` for "no chord" and "some other chord".
pub fn chord_of(class: usize) -> Option<Chord> {
    (class < 168).then(|| {
        let root = (class / 14) as u8;
        Chord { root, quality: QUALITIES[class % 14], bass: root }
    })
}

struct Block {
    norm1: (Vec<f32>, Vec<f32>),
    query: Vec<f32>,
    key: Vec<f32>,
    value: Vec<f32>,
    output: Vec<f32>,
    norm2: (Vec<f32>, Vec<f32>),
    /// The two kernel-3 convolutions, as [128·3, 128].
    conv: [(Vec<f32>, Vec<f32>); 2],
    mask: Mask,
}

struct Layer {
    fwd: Block,
    bwd: Block,
    merge: Linear,
}

#[derive(Default)]
struct Scratch {
    norm: Vec<f32>,
    q: Vec<f32>,
    k: Vec<f32>,
    v: Vec<f32>,
    qh: Vec<f32>,
    kh: Vec<f32>,
    vh: Vec<f32>,
    oh: Vec<f32>,
    merged: Vec<f32>,
    proj: Vec<f32>,
    cols: Vec<f32>,
    hidden: Vec<f32>,
    both: Vec<f32>,
    attend: AttendScratch,
}

pub struct Btc {
    pub mean: f32,
    pub std: f32,
    embed: Vec<f32>,
    timing: Vec<f32>,
    layers: Vec<Layer>,
    norm: (Vec<f32>, Vec<f32>),
    out: Linear,
    scratch: Scratch,
}

impl Block {
    fn load(w: &mut Weights, p: &str, mask: Mask) -> Result<Self, String> {
        let mut t = |n: &str, shape: &[usize]| w.take(&format!("{p}{n}"), shape);
        let norm1 = (t("norm1.gamma", &[DIM])?, t("norm1.beta", &[DIM])?);
        let (query, key, value, output) = (t("query", &[DIM, DIM])?, t("key", &[DIM, DIM])?, t("value", &[DIM, DIM])?, t("output", &[DIM, DIM])?);
        let norm2 = (t("norm2.gamma", &[DIM])?, t("norm2.beta", &[DIM])?);
        let conv0 = (conv_matrix(&t("conv0.weight", &[DIM, DIM, 3])?, DIM), t("conv0.bias", &[DIM])?);
        let conv1 = (conv_matrix(&t("conv1.weight", &[DIM, DIM, 3])?, DIM), t("conv1.bias", &[DIM])?);
        Ok(Self { norm1, query, key, value, output, norm2, conv: [conv0, conv1], mask })
    }

    /// The block's output for `x` [n, DIM], into `out`.
    fn run(&self, x: &[f32], n: usize, s: &mut Scratch, out: &mut Vec<f32>) {
        layer_norm(x, DIM, &self.norm1.0, &self.norm1.1, 1e-6, &mut s.norm);
        for (w, o) in [(&self.query, &mut s.q), (&self.key, &mut s.k), (&self.value, &mut s.v)] {
            o.resize(n * DIM, 0.0);
            matmul(&s.norm, n, DIM, w, DIM, None, o);
        }
        let scale = (HEAD as f32).powf(-0.5);
        for b in [&mut s.qh, &mut s.kh, &mut s.vh, &mut s.oh] {
            b.resize(n * HEAD, 0.0);
        }
        s.merged.resize(n * DIM, 0.0);
        for h in 0..HEADS {
            for i in 0..n {
                for j in 0..HEAD {
                    s.qh[i * HEAD + j] = s.q[i * DIM + h * HEAD + j] * scale;
                    s.kh[i * HEAD + j] = s.k[i * DIM + h * HEAD + j];
                    s.vh[i * HEAD + j] = s.v[i * DIM + h * HEAD + j];
                }
            }
            attend(&s.qh, &s.kh, &s.vh, n, HEAD, self.mask, &mut s.attend, &mut s.oh);
            for i in 0..n {
                s.merged[i * DIM + h * HEAD..i * DIM + (h + 1) * HEAD].copy_from_slice(&s.oh[i * HEAD..(i + 1) * HEAD]);
            }
        }
        s.proj.resize(n * DIM, 0.0);
        matmul(&s.merged, n, DIM, &self.output, DIM, None, &mut s.proj);
        // x + attention, then x + the feed-forward of its norm.
        out.resize(n * DIM, 0.0);
        for i in 0..n * DIM {
            out[i] = x[i] + s.proj[i];
        }
        layer_norm(out, DIM, &self.norm2.0, &self.norm2.1, 1e-6, &mut s.norm);
        let mut input = std::mem::take(&mut s.norm);
        for (w, b) in &self.conv {
            conv3(&input, n, &mut s.cols);
            s.hidden.resize(n * DIM, 0.0);
            matmul(&s.cols, n, DIM * 3, w, DIM, Some(b), &mut s.hidden);
            relu(&mut s.hidden);
            std::mem::swap(&mut input, &mut s.hidden);
        }
        for i in 0..n * DIM {
            out[i] += input[i];
        }
        s.norm = input;
    }
}

/// Im2col for a kernel-3 convolution over time, padded by one each side,
/// columns ordered (channel, tap) like PyTorch's [out, in, 3] weights.
fn conv3(x: &[f32], n: usize, out: &mut Vec<f32>) {
    out.resize(n * DIM * 3, 0.0);
    for t in 0..n {
        let row = &mut out[t * DIM * 3..(t + 1) * DIM * 3];
        for k in 0..3 {
            let src = t as isize + k as isize - 1;
            for c in 0..DIM {
                row[c * 3 + k] = if src >= 0 && (src as usize) < n { x[src as usize * DIM + c] } else { 0.0 };
            }
        }
    }
}

impl Btc {
    pub fn load(bytes: &[u8]) -> Result<Self, String> {
        let mut w = Weights::parse(bytes)?;
        let mean = w.take("mean", &[1])?[0];
        let std = w.take("std", &[1])?[0];
        let embed = w.take("embed", &[BINS, DIM])?;
        let mut layers = Vec::with_capacity(LAYERS);
        for i in 0..LAYERS {
            layers.push(Layer {
                fwd: Block::load(&mut w, &format!("layer{i}.fwd."), Mask::Past)?,
                bwd: Block::load(&mut w, &format!("layer{i}.bwd."), Mask::Future)?,
                merge: Linear { w: w.take(&format!("layer{i}.merge.weight"), &[2 * DIM, DIM])?, b: Some(w.take(&format!("layer{i}.merge.bias"), &[DIM])?), n_in: 2 * DIM, n_out: DIM },
            });
        }
        let norm = (w.take("norm.gamma", &[DIM])?, w.take("norm.beta", &[DIM])?);
        let out = Linear { w: w.take("out.weight", &[DIM, CLASSES])?, b: Some(w.take("out.bias", &[CLASSES])?), n_in: DIM, n_out: CLASSES };
        // The sinusoid timing signal: sines over the first half, cosines over the second.
        let half = DIM / 2;
        let step = (1.0e4f64).ln() / (half - 1) as f64;
        let mut timing = vec![0.0; FRAMES * DIM];
        for t in 0..FRAMES {
            for i in 0..half {
                let a = t as f64 * (-(i as f64) * step).exp();
                timing[t * DIM + i] = a.sin() as f32;
                timing[t * DIM + half + i] = a.cos() as f32;
            }
        }
        Ok(Self { mean, std, embed, timing, layers, norm, out, scratch: Scratch::default() })
    }

    /// Chord logits [n, 170] for `n` (at most 108) frames of log-CQT [n, 144].
    pub fn run(&mut self, cqt: &[f32], n: usize) -> Vec<f32> {
        assert!(n <= FRAMES);
        let mut s = std::mem::take(&mut self.scratch);
        let normed: Vec<f32> = cqt[..n * BINS].iter().map(|v| (v - self.mean) / self.std).collect();
        let mut x = vec![0.0; n * DIM];
        matmul(&normed, n, BINS, &self.embed, DIM, None, &mut x);
        for (v, t) in x.iter_mut().zip(&self.timing) {
            *v += t;
        }
        let (mut a, mut b) = (Vec::new(), Vec::new());
        for layer in &self.layers {
            layer.fwd.run(&x, n, &mut s, &mut a);
            layer.bwd.run(&x, n, &mut s, &mut b);
            s.both.resize(n * 2 * DIM, 0.0);
            for t in 0..n {
                s.both[t * 2 * DIM..t * 2 * DIM + DIM].copy_from_slice(&a[t * DIM..(t + 1) * DIM]);
                s.both[t * 2 * DIM + DIM..(t + 1) * 2 * DIM].copy_from_slice(&b[t * DIM..(t + 1) * DIM]);
            }
            layer.merge.forward(&s.both, n, &mut x);
        }
        layer_norm(&x, DIM, &self.norm.0, &self.norm.1, 1e-6, &mut s.norm);
        let mut logits = Vec::new();
        self.out.forward(&s.norm, n, &mut logits);
        self.scratch = s;
        logits
    }
}
