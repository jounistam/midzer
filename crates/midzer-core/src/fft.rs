//! A small FFT shared by the listeners, allocated once up front.

/// In-place radix-2 complex FFT of a fixed size, with precomputed tables.
pub(crate) struct Fft {
    cos: Box<[f32]>,
    sin: Box<[f32]>,
    reversed: Box<[u16]>,
}

impl Fft {
    pub(crate) fn new(n: usize) -> Self {
        let bits = n.trailing_zeros();
        let half = n / 2;
        let angle = |k: usize| -2.0 * std::f64::consts::PI * k as f64 / n as f64;
        Self {
            cos: (0..half).map(|k| angle(k).cos() as f32).collect(),
            sin: (0..half).map(|k| angle(k).sin() as f32).collect(),
            reversed: (0..n).map(|i| ((i as u32).reverse_bits() >> (32 - bits)) as u16).collect(),
        }
    }

    pub(crate) fn run(&self, re: &mut [f32], im: &mut [f32]) {
        let n = re.len();
        for i in 0..n {
            let j = self.reversed[i] as usize;
            if j > i {
                re.swap(i, j);
                im.swap(i, j);
            }
        }
        let mut size = 2;
        while size <= n {
            let half = size / 2;
            let step = n / size;
            for start in (0..n).step_by(size) {
                for k in 0..half {
                    let (c, s) = (self.cos[k * step], self.sin[k * step]);
                    let (a, b) = (start + k, start + k + half);
                    let tr = re[b] * c - im[b] * s;
                    let ti = re[b] * s + im[b] * c;
                    re[b] = re[a] - tr;
                    im[b] = im[a] - ti;
                    re[a] += tr;
                    im[a] += ti;
                }
            }
            size *= 2;
        }
    }
}
