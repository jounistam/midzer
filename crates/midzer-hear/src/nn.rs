//! The few network operations the two models need, on plain row-major
//! slices: matrix products, norms, activations and attention.

/// Rows and columns of the register tile.
const TR: usize = 4;
const TC: usize = 8;

/// `out[r][j] = Σ_p x[r][p] · w[p][j]` (plus `bias[j]`), for `rows` rows of
/// `k` inputs and `m` outputs. A tile of 4 rows × 8 columns is summed in
/// registers over all of `k`; the edges four rows at a time.
pub fn matmul(x: &[f32], rows: usize, k: usize, w: &[f32], m: usize, bias: Option<&[f32]>, out: &mut [f32]) {
    debug_assert!(x.len() >= rows * k && w.len() >= k * m && out.len() >= rows * m);
    let cols = m - m % TC;
    let mut r = 0;
    while r + TR <= rows {
        let xs: [&[f32]; TR] = std::array::from_fn(|i| &x[(r + i) * k..(r + i + 1) * k]);
        let mut j = 0;
        while j < cols {
            let mut acc = [[0.0f32; TC]; TR];
            if let Some(b) = bias {
                for a in acc.iter_mut() {
                    a.copy_from_slice(&b[j..j + TC]);
                }
            }
            for p in 0..k {
                let wv: &[f32; TC] = w[p * m + j..p * m + j + TC].try_into().unwrap();
                for i in 0..TR {
                    let a = xs[i][p];
                    for c in 0..TC {
                        acc[i][c] += a * wv[c];
                    }
                }
            }
            for i in 0..TR {
                out[(r + i) * m + j..(r + i) * m + j + TC].copy_from_slice(&acc[i]);
            }
            j += TC;
        }
        r += TR;
    }
    if cols < m {
        edge(x, 0, r, k, w, m, cols, bias, out);
    }
    if r < rows {
        edge(x, r, rows, k, w, m, 0, bias, out);
    }
}

/// Rows `from..to` of the product, columns `col..m`.
fn edge(x: &[f32], from: usize, to: usize, k: usize, w: &[f32], m: usize, col: usize, bias: Option<&[f32]>, out: &mut [f32]) {
    for row in from..to {
        let o = &mut out[row * m + col..(row + 1) * m];
        match bias {
            Some(b) => o.copy_from_slice(&b[col..m]),
            None => o.fill(0.0),
        }
    }
    let mut r = from;
    while r + 4 <= to {
        let (o0, rest) = out[r * m..(r + 4) * m].split_at_mut(m);
        let (o1, rest) = rest.split_at_mut(m);
        let (o2, o3) = rest.split_at_mut(m);
        let (x0, x1, x2, x3) = (&x[r * k..], &x[(r + 1) * k..], &x[(r + 2) * k..], &x[(r + 3) * k..]);
        for p in 0..k {
            let wr = &w[p * m..(p + 1) * m];
            let (a0, a1, a2, a3) = (x0[p], x1[p], x2[p], x3[p]);
            for j in col..m {
                let v = wr[j];
                o0[j] += a0 * v;
                o1[j] += a1 * v;
                o2[j] += a2 * v;
                o3[j] += a3 * v;
            }
        }
        r += 4;
    }
    while r < to {
        let o = &mut out[r * m..(r + 1) * m];
        let xr = &x[r * k..(r + 1) * k];
        for p in 0..k {
            let a = xr[p];
            let wr = &w[p * m..(p + 1) * m];
            for j in col..m {
                o[j] += a * wr[j];
            }
        }
        r += 1;
    }
}

/// A fully connected layer with its weight stored as [in, out].
pub struct Linear {
    pub w: Vec<f32>,
    pub b: Option<Vec<f32>>,
    pub n_in: usize,
    pub n_out: usize,
}

impl Linear {
    pub fn forward(&self, x: &[f32], rows: usize, out: &mut Vec<f32>) {
        out.resize(rows * self.n_out, 0.0);
        matmul(x, rows, self.n_in, &self.w, self.n_out, self.b.as_deref(), out);
    }
}

/// `x / max(‖x‖, 1e-12) · √dim · γ` per row (the RoFormer's RMS norm).
pub fn rms_norm(x: &[f32], dim: usize, gamma: &[f32], out: &mut Vec<f32>) {
    out.resize(x.len(), 0.0);
    let scale = (dim as f32).sqrt();
    for (row, o) in x.chunks_exact(dim).zip(out.chunks_exact_mut(dim)) {
        let norm = row.iter().map(|v| v * v).sum::<f32>().sqrt().max(1e-12);
        let s = scale / norm;
        for j in 0..dim {
            o[j] = row[j] * s * gamma[j];
        }
    }
}

/// Layer norm as BTC has it: the standard deviation with n − 1, and ε added to it.
pub fn layer_norm(x: &[f32], dim: usize, gamma: &[f32], beta: &[f32], eps: f32, out: &mut Vec<f32>) {
    out.resize(x.len(), 0.0);
    for (row, o) in x.chunks_exact(dim).zip(out.chunks_exact_mut(dim)) {
        let mean = row.iter().sum::<f32>() / dim as f32;
        let var = row.iter().map(|v| (v - mean) * (v - mean)).sum::<f32>() / (dim - 1) as f32;
        let inv = 1.0 / (var.sqrt() + eps);
        for j in 0..dim {
            o[j] = gamma[j] * (row[j] - mean) * inv + beta[j];
        }
    }
}

pub fn gelu(x: &mut [f32]) {
    for v in x {
        *v = 0.5 * *v * (1.0 + erf(*v * std::f32::consts::FRAC_1_SQRT_2));
    }
}

pub fn relu(x: &mut [f32]) {
    for v in x {
        *v = v.max(0.0);
    }
}

pub fn sigmoid(x: f32) -> f32 {
    1.0 / (1.0 + (-x).exp())
}

/// The error function (Abramowitz and Stegun 7.1.26, within 1.5e-7).
pub fn erf(x: f32) -> f32 {
    let z = x.abs();
    let t = 1.0 / (1.0 + 0.327_591_1 * z);
    let poly = t * (0.254_829_6 + t * (-0.284_496_74 + t * (1.421_413_7 + t * (-1.453_152 + t * 1.061_405_4))));
    let r = 1.0 - poly * (-z * z).exp();
    if x < 0.0 { -r } else { r }
}

/// Softmax of each row in place.
pub fn softmax_rows(x: &mut [f32], width: usize) {
    for row in x.chunks_exact_mut(width) {
        let max = row.iter().fold(f32::NEG_INFINITY, |m, &v| m.max(v));
        if max == f32::NEG_INFINITY {
            row.fill(0.0);
            continue;
        }
        let mut sum = 0.0;
        for v in row.iter_mut() {
            *v = (*v - max).exp();
            sum += *v;
        }
        let inv = 1.0 / sum;
        for v in row.iter_mut() {
            *v *= inv;
        }
    }
}

/// Which keys each query may see.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Mask {
    None,
    /// Only itself and earlier ones.
    Past,
    /// Only itself and later ones.
    Future,
}

/// Scratch space for [`attend`], kept between calls.
#[derive(Default)]
pub struct AttendScratch {
    kt: Vec<f32>,
    scores: Vec<f32>,
}

/// Scaled dot-product attention of one head over one sequence: `q`, `k` and
/// `v` are [n, d]; the result goes to `out` [n, d]. `q` is already scaled.
pub fn attend(q: &[f32], k: &[f32], v: &[f32], n: usize, d: usize, mask: Mask, s: &mut AttendScratch, out: &mut [f32]) {
    // Kᵀ as [d, n], so that Q·Kᵀ is an ordinary product.
    s.kt.resize(d * n, 0.0);
    for i in 0..n {
        for j in 0..d {
            s.kt[j * n + i] = k[i * d + j];
        }
    }
    s.scores.resize(n * n, 0.0);
    matmul(q, n, d, &s.kt, n, None, &mut s.scores);
    match mask {
        Mask::None => {}
        Mask::Past => {
            for i in 0..n {
                s.scores[i * n + i + 1..(i + 1) * n].fill(f32::NEG_INFINITY);
            }
        }
        Mask::Future => {
            for i in 0..n {
                s.scores[i * n..i * n + i].fill(f32::NEG_INFINITY);
            }
        }
    }
    softmax_rows(&mut s.scores, n);
    matmul(&s.scores, n, n, v, d, None, out);
}

/// Im2col for a 2-D convolution over a [F][T][C] input (channels last), with
/// a kernel of (kf, kt), stride (sf, 1) and padding (0, kt / 2) in time:
/// one row per output position (fo, t), ordered (c, kf, kt) like PyTorch's
/// [out, in, kf, kt] weights.
pub fn im2col(x: &[f32], f: usize, t: usize, c: usize, kf: usize, kt: usize, sf: usize, out: &mut Vec<f32>) -> usize {
    let fo = (f - kf) / sf + 1;
    let width = c * kf * kt;
    out.resize(fo * t * width, 0.0);
    let pad = (kt / 2) as isize;
    for o in 0..fo {
        for ti in 0..t {
            let row = &mut out[(o * t + ti) * width..(o * t + ti + 1) * width];
            for a in 0..kf {
                let fi = o * sf + a;
                for b in 0..kt {
                    let tt = ti as isize + b as isize - pad;
                    for ci in 0..c {
                        row[ci * kf * kt + a * kt + b] = if tt >= 0 && (tt as usize) < t { x[(fi * t + tt as usize) * c + ci] } else { 0.0 };
                    }
                }
            }
        }
    }
    fo
}

/// PyTorch conv weights [out, in, kf, kt] as a matrix [in·kf·kt, out].
pub fn conv_matrix(w: &[f32], out_c: usize) -> Vec<f32> {
    let width = w.len() / out_c;
    let mut m = vec![0.0; w.len()];
    for o in 0..out_c {
        for i in 0..width {
            m[i * out_c + o] = w[o * width + i];
        }
    }
    m
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matmul_matches_the_definition() {
        for (rows, k, m) in [(7, 5, 3), (9, 6, 19), (8, 3, 16)] {
        let x: Vec<f32> = (0..rows * k).map(|i| (i as f32 * 0.37).sin()).collect();
        let w: Vec<f32> = (0..k * m).map(|i| (i as f32 * 0.91).cos()).collect();
        let b: Vec<f32> = (0..m).map(|j| j as f32 * 0.5 - 1.0).collect();
        let mut out = vec![0.0; rows * m];
        matmul(&x, rows, k, &w, m, Some(&b), &mut out);
        for r in 0..rows {
            for j in 0..m {
                let want: f32 = b[j] + (0..k).map(|p| x[r * k + p] * w[p * m + j]).sum::<f32>();
                assert!((out[r * m + j] - want).abs() < 1e-5);
            }
        }
        }
    }

    #[test]
    fn erf_is_close() {
        for (x, want) in [(0.0, 0.0), (0.5, 0.520_499_9), (1.0, 0.842_700_8), (-2.0, -0.995_322_3), (3.0, 0.999_977_9)] {
            assert!((erf(x) - want).abs() < 3e-7, "erf({x}) = {}", erf(x));
        }
    }

    #[test]
    fn masked_attention_sees_only_one_side() {
        let (n, d) = (3, 2);
        let q = vec![0.0; n * d];
        let k = vec![0.0; n * d];
        let v = vec![1.0, 0.0, 0.0, 1.0, 1.0, 1.0];
        let mut s = AttendScratch::default();
        let mut out = vec![0.0; n * d];
        attend(&q, &k, &v, n, d, Mask::Past, &mut s, &mut out);
        // Equal scores: the mean of the values allowed.
        assert_eq!(&out[..2], &[1.0, 0.0]);
        assert!((out[2] - 0.5).abs() < 1e-6 && (out[3] - 0.5).abs() < 1e-6);
        attend(&q, &k, &v, n, d, Mask::Future, &mut s, &mut out);
        assert_eq!(&out[4..], &[1.0, 1.0]);
    }
}
