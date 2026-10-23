//! Beat This! (Foscarin, Schlüter and Widmer, ISMIR 2024), the small model:
//! beat and downbeat logits, 50 a second, from a log-mel spectrogram (see
//! [`crate::mel`]). A convolutional stem, three blocks of a convolution with
//! small transformers along frequency and along time, then six RoFormer
//! layers over time.
//!
//! The network sees a whole window at once, both ways in time, so the frames
//! near its end are the least sure; see [`crate::hearing`].

use crate::mzm::Weights;
use crate::nn::{AttendScratch, Linear, Mask, attend, conv_matrix, gelu, im2col, matmul, rms_norm, sigmoid};

pub const MELS: usize = 128;
/// Frames a second.
pub const FPS: f32 = 50.0;
const DIM: usize = 128;
const HEAD: usize = 32;
const LAYERS: usize = 6;
const STEM: usize = 32;

/// Multi-head attention with rotary positions and a sigmoid gate per head.
struct Attention {
    norm: Vec<f32>,
    qkv: Linear,
    gates: Linear,
    out: Linear,
    dim: usize,
    heads: usize,
}

struct FeedForward {
    norm: Vec<f32>,
    up: Linear,
    down: Linear,
    dim: usize,
}

fn linear(w: &mut Weights, name: &str, n_in: usize, n_out: usize, bias: bool) -> Result<Linear, String> {
    let (wn, bn) = if bias { (format!("{name}.weight"), Some(format!("{name}.bias"))) } else { (name.to_string(), None) };
    Ok(Linear { w: w.take(&wn, &[n_in, n_out])?, b: bn.map(|b| w.take(&b, &[n_out])).transpose()?, n_in, n_out })
}

impl Attention {
    fn load(w: &mut Weights, p: &str, dim: usize) -> Result<Self, String> {
        let heads = dim / HEAD;
        Ok(Self {
            norm: w.take(&format!("{p}.norm"), &[dim])?,
            qkv: linear(w, &format!("{p}.qkv"), dim, 3 * dim, false)?,
            gates: linear(w, &format!("{p}.gates"), dim, heads, true)?,
            out: linear(w, &format!("{p}.out"), dim, dim, false)?,
            dim,
            heads,
        })
    }

    /// Adds the attention of each of `seqs` sequences of `n` tokens (stored
    /// one after another in `x`) to `x`.
    fn add(&self, x: &mut [f32], n: usize, seqs: usize, s: &mut Scratch) {
        let (dim, heads) = (self.dim, self.heads);
        let rows = n * seqs;
        rms_norm(x, dim, &self.norm, &mut s.norm);
        self.qkv.forward(&s.norm, rows, &mut s.qkv);
        self.gates.forward(&s.norm, rows, &mut s.gates);
        s.merged.resize(rows * dim, 0.0);
        s.rotary(n);
        let scale = (HEAD as f32).powf(-0.5);
        for q in [&mut s.qh, &mut s.kh, &mut s.vh, &mut s.oh] {
            q.resize(n * HEAD, 0.0);
        }
        for seq in 0..seqs {
            for h in 0..heads {
                for i in 0..n {
                    let row = &s.qkv[(seq * n + i) * 3 * dim..];
                    for j in 0..HEAD {
                        s.qh[i * HEAD + j] = row[h * HEAD + j];
                        s.kh[i * HEAD + j] = row[dim + h * HEAD + j];
                        s.vh[i * HEAD + j] = row[2 * dim + h * HEAD + j];
                    }
                }
                rotate(&mut s.qh, n, &s.cos, &s.sin);
                rotate(&mut s.kh, n, &s.cos, &s.sin);
                for v in s.qh.iter_mut() {
                    *v *= scale;
                }
                attend(&s.qh, &s.kh, &s.vh, n, HEAD, Mask::None, &mut s.attend, &mut s.oh);
                for i in 0..n {
                    let token = seq * n + i;
                    let gate = sigmoid(s.gates[token * heads + h]);
                    for j in 0..HEAD {
                        s.merged[token * dim + h * HEAD + j] = s.oh[i * HEAD + j] * gate;
                    }
                }
            }
        }
        self.out.forward(&s.merged, rows, &mut s.proj);
        for (a, b) in x.iter_mut().zip(&s.proj) {
            *a += b;
        }
    }
}

impl FeedForward {
    fn load(w: &mut Weights, p: &str, dim: usize) -> Result<Self, String> {
        Ok(Self {
            norm: w.take(&format!("{p}.norm"), &[dim])?,
            up: linear(w, &format!("{p}.up"), dim, 4 * dim, true)?,
            down: linear(w, &format!("{p}.down"), 4 * dim, dim, true)?,
            dim,
        })
    }

    fn add(&self, x: &mut [f32], s: &mut Scratch) {
        let rows = x.len() / self.dim;
        rms_norm(x, self.dim, &self.norm, &mut s.norm);
        self.up.forward(&s.norm, rows, &mut s.hidden);
        gelu(&mut s.hidden);
        self.down.forward(&s.hidden, rows, &mut s.proj);
        for (a, b) in x.iter_mut().zip(&s.proj) {
            *a += b;
        }
    }
}

/// Rotary positions on one head [n, HEAD], pairs interleaved.
fn rotate(x: &mut [f32], n: usize, cos: &[f32], sin: &[f32]) {
    let half = HEAD / 2;
    for pos in 0..n {
        for i in 0..half {
            let (c, s) = (cos[pos * half + i], sin[pos * half + i]);
            let at = pos * HEAD + 2 * i;
            let (a, b) = (x[at], x[at + 1]);
            x[at] = a * c - b * s;
            x[at + 1] = b * c + a * s;
        }
    }
}

struct Block {
    attn_f: Attention,
    ff_f: FeedForward,
    attn_t: Attention,
    ff_t: FeedForward,
    /// The convolution (2, 3), stride (2, 1), as [c·2·3, 2c], with its batch norm folded in.
    conv: Vec<f32>,
    bias: Vec<f32>,
    c: usize,
}

struct Layer {
    attn: Attention,
    ff: FeedForward,
}

/// Buffers reused between runs.
#[derive(Default)]
struct Scratch {
    norm: Vec<f32>,
    qkv: Vec<f32>,
    gates: Vec<f32>,
    merged: Vec<f32>,
    proj: Vec<f32>,
    hidden: Vec<f32>,
    qh: Vec<f32>,
    kh: Vec<f32>,
    vh: Vec<f32>,
    oh: Vec<f32>,
    attend: AttendScratch,
    cos: Vec<f32>,
    sin: Vec<f32>,
    rotary_len: usize,
    a: Vec<f32>,
    b: Vec<f32>,
    cols: Vec<f32>,
}

impl Scratch {
    /// Sine and cosine tables for positions up to `n`.
    fn rotary(&mut self, n: usize) {
        if self.rotary_len >= n {
            return;
        }
        let half = HEAD / 2;
        self.cos.resize(n * half, 0.0);
        self.sin.resize(n * half, 0.0);
        for pos in 0..n {
            for i in 0..half {
                let f = 1.0 / 10_000f64.powf(2.0 * i as f64 / HEAD as f64);
                let (s, c) = (pos as f64 * f).sin_cos();
                self.cos[pos * half + i] = c as f32;
                self.sin[pos * half + i] = s as f32;
            }
        }
        self.rotary_len = n;
    }
}

pub struct BeatThis {
    scale: Vec<f32>,
    offset: Vec<f32>,
    stem: Vec<f32>,
    stem_bias: Vec<f32>,
    blocks: Vec<Block>,
    linear: Linear,
    layers: Vec<Layer>,
    norm: Vec<f32>,
    head: Linear,
    /// The input's mel filterbank, [513, 128].
    pub mel_filters: Vec<f32>,
    scratch: Scratch,
}

/// What the network makes of a window: logits per frame.
pub struct Logits {
    pub beat: Vec<f32>,
    pub downbeat: Vec<f32>,
}

impl BeatThis {
    pub fn load(bytes: &[u8]) -> Result<Self, String> {
        let mut w = Weights::parse(bytes)?;
        let mut blocks = Vec::new();
        let mut c = STEM;
        for i in 0..3 {
            blocks.push(Block {
                attn_f: Attention::load(&mut w, &format!("block{i}.attn_f"), c)?,
                ff_f: FeedForward::load(&mut w, &format!("block{i}.ff_f"), c)?,
                attn_t: Attention::load(&mut w, &format!("block{i}.attn_t"), c)?,
                ff_t: FeedForward::load(&mut w, &format!("block{i}.ff_t"), c)?,
                conv: conv_matrix(&w.take(&format!("block{i}.conv.weight"), &[2 * c, c, 2, 3])?, 2 * c),
                bias: w.take(&format!("block{i}.conv.bias"), &[2 * c])?,
                c,
            });
            c *= 2;
        }
        let layers = (0..LAYERS)
            .map(|i| Ok(Layer { attn: Attention::load(&mut w, &format!("layer{i}.attn"), DIM)?, ff: FeedForward::load(&mut w, &format!("layer{i}.ff"), DIM)? }))
            .collect::<Result<_, String>>()?;
        Ok(Self {
            scale: w.take("stem.scale", &[MELS])?,
            offset: w.take("stem.offset", &[MELS])?,
            stem: conv_matrix(&w.take("stem.conv.weight", &[STEM, 1, 4, 3])?, STEM),
            stem_bias: w.take("stem.conv.bias", &[STEM])?,
            blocks,
            linear: linear(&mut w, "linear", 256 * 4, DIM, true)?,
            layers,
            norm: w.take("norm", &[DIM])?,
            head: linear(&mut w, "head", DIM, 2, true)?,
            mel_filters: w.take("mel.filters", &[513, MELS])?,
            scratch: Scratch::default(),
        })
    }

    /// Runs the network over `frames` frames of the log-mel spectrogram `mel` [frames, 128].
    pub fn run(&mut self, mel: &[f32], frames: usize) -> Logits {
        let t = frames;
        let mut s = std::mem::take(&mut self.scratch);
        // The input batch norm, into [F][T] with one channel.
        s.a.resize(MELS * t, 0.0);
        for ti in 0..t {
            for f in 0..MELS {
                s.a[f * t + ti] = mel[ti * MELS + f] * self.scale[f] + self.offset[f];
            }
        }
        let mut f = im2col(&s.a, MELS, t, 1, 4, 3, 4, &mut s.cols);
        s.a.resize(f * t * STEM, 0.0);
        matmul(&s.cols, f * t, 12, &self.stem, STEM, Some(&self.stem_bias), &mut s.a);
        gelu(&mut s.a);

        for block in &self.blocks {
            let c = block.c;
            // Along frequency: T sequences of F tokens, so [T][F][C].
            transpose_ft(&s.a, f, t, c, &mut s.b);
            let mut x = std::mem::take(&mut s.b);
            block.attn_f.add(&mut x, f, t, &mut s);
            block.ff_f.add(&mut x, &mut s);
            transpose_tf(&x, t, f, c, &mut s.a);
            s.b = x;
            // Along time: F sequences of T tokens, [F][T][C] as it is.
            let mut x = std::mem::take(&mut s.a);
            block.attn_t.add(&mut x, t, f, &mut s);
            block.ff_t.add(&mut x, &mut s);
            let fo = im2col(&x, f, t, c, 2, 3, 2, &mut s.cols);
            s.a = x;
            s.a.resize(fo * t * 2 * c, 0.0);
            matmul(&s.cols, fo * t, c * 6, &block.conv, 2 * c, Some(&block.bias), &mut s.a);
            gelu(&mut s.a);
            f = fo;
        }

        // [F=4][T][C=256] to [T][(C F)].
        let c = 256;
        s.b.resize(t * c * f, 0.0);
        for fi in 0..f {
            for ti in 0..t {
                for ci in 0..c {
                    s.b[ti * c * f + ci * f + fi] = s.a[(fi * t + ti) * c + ci];
                }
            }
        }
        let mut x = Vec::new();
        self.linear.forward(&s.b, t, &mut x);
        for layer in &self.layers {
            layer.attn.add(&mut x, t, 1, &mut s);
            layer.ff.add(&mut x, &mut s);
        }
        rms_norm(&x, DIM, &self.norm, &mut s.norm);
        let mut out = Vec::new();
        self.head.forward(&s.norm, t, &mut out);
        self.scratch = s;
        let (pairs, _) = out.as_chunks::<2>();
        Logits { beat: pairs.iter().map(|p| p[0] + p[1]).collect(), downbeat: pairs.iter().map(|p| p[1]).collect() }
    }
}

/// [F][T][C] to [T][F][C].
fn transpose_ft(x: &[f32], f: usize, t: usize, c: usize, out: &mut Vec<f32>) {
    out.resize(f * t * c, 0.0);
    for fi in 0..f {
        for ti in 0..t {
            out[(ti * f + fi) * c..(ti * f + fi + 1) * c].copy_from_slice(&x[(fi * t + ti) * c..(fi * t + ti + 1) * c]);
        }
    }
}

/// [T][F][C] to [F][T][C].
fn transpose_tf(x: &[f32], t: usize, f: usize, c: usize, out: &mut Vec<f32>) {
    out.resize(f * t * c, 0.0);
    for ti in 0..t {
        for fi in 0..f {
            out[(fi * t + ti) * c..(fi * t + ti + 1) * c].copy_from_slice(&x[(ti * f + fi) * c..(ti * f + fi + 1) * c]);
        }
    }
}
