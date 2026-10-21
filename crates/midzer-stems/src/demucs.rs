//! Stem separation with HT-Demucs: 7.8-second stereo segments at 44.1 kHz,
//! overlapped by a quarter and cross-faded, as in the model's reference code.

use ort::session::Session;
use ort::value::Tensor;

pub const RATE: u32 = 44_100;
const SEGMENT: usize = 343_980;
pub const STEMS: [&str; 4] = ["drums", "bass", "other", "vocals"];

/// Separated stems, each left and right.
pub struct Stems {
    pub stems: [[Vec<f32>; 2]; 4],
}

impl Stems {
    pub fn mono(&self, stem: usize) -> Vec<f32> {
        let [l, r] = &self.stems[stem];
        l.iter().zip(r).map(|(a, b)| 0.5 * (a + b)).collect()
    }
}

pub fn separate(session: &mut Session, left: &[f32], right: &[f32], progress: &mut dyn FnMut(f32) -> bool) -> Result<Stems, String> {
    let total = left.len();
    let overlap = SEGMENT / 4;
    let stride = SEGMENT - overlap;
    let chunks = total.div_ceil(stride).max(1);
    let window: Vec<f32> = (0..SEGMENT)
        .map(|i| {
            let fade = |k: usize| k as f32 / (overlap - 1) as f32;
            if i < overlap { fade(i) } else if i >= SEGMENT - overlap { fade(SEGMENT - 1 - i) } else { 1.0 }
        })
        .collect();
    let mut stems: [[Vec<f32>; 2]; 4] = std::array::from_fn(|_| [vec![0.0; total], vec![0.0; total]]);
    let mut weight = vec![0.0f32; total];
    for chunk in 0..chunks {
        let start = chunk * stride;
        let end = (start + SEGMENT).min(total);
        let mut input = vec![0.0f32; 2 * SEGMENT];
        input[..end - start].copy_from_slice(&left[start..end]);
        input[SEGMENT..SEGMENT + end - start].copy_from_slice(&right[start..end]);
        let tensor = Tensor::from_array(([1usize, 2, SEGMENT], input)).map_err(|e| e.to_string())?;
        let outputs = session.run(ort::inputs!["mix" => tensor]).map_err(|e| e.to_string())?;
        let (_, out) = outputs["stems"].try_extract_tensor::<f32>().map_err(|e| e.to_string())?;
        for (s, stem) in stems.iter_mut().enumerate() {
            for (c, channel) in stem.iter_mut().enumerate() {
                let src = &out[(s * 2 + c) * SEGMENT..];
                for i in 0..end - start {
                    channel[start + i] += src[i] * window[i];
                }
            }
        }
        for i in 0..end - start {
            weight[start + i] += window[i];
        }
        if !progress((chunk + 1) as f32 / chunks as f32) {
            return Err(crate::CANCELLED.into());
        }
    }
    for stem in &mut stems {
        for channel in stem {
            for (v, w) in channel.iter_mut().zip(&weight) {
                *v /= w.max(1e-8);
            }
        }
    }
    Ok(Stems { stems })
}
