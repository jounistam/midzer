//! Recordings to play along with: decoded by the same decoder as the
//! analysis, so the beat times it found line up with what is heard, and
//! resampled to the audio output's rate for the engine (`midzer_core::Track`).

use midzer_core::Track;

use crate::{decode, resample};

/// Interleaves two channels as 16-bit samples, `len` frames long (padded with silence).
pub fn interleave(left: &[f32], right: &[f32], len: usize) -> Vec<i16> {
    let sample = |x: Option<&f32>| (x.copied().unwrap_or(0.0).clamp(-1.0, 1.0) * 32_767.0).round() as i16;
    (0..len).flat_map(|i| [sample(left.get(i)), sample(right.get(i))]).collect()
}

/// Decodes a recording and resamples it to `rate`, one channel at a time to keep the peak down.
pub fn decode_at(bytes: Vec<u8>, rate: u32) -> Result<(Vec<f32>, Vec<f32>), String> {
    let decoded = decode(bytes)?;
    let from = decoded.sample_rate;
    let (left, right) = (decoded.left, decoded.right);
    let left = resample(&left, from, rate);
    let right = resample(&right, from, rate);
    Ok((left, right))
}

/// A recording as the engine plays it.
pub fn backing_track(bytes: Vec<u8>, rate: u32) -> Result<Track, String> {
    let (left, right) = decode_at(bytes, rate)?;
    Ok(Track { main: interleave(&left, &right, left.len()), drums: None })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A 16-bit stereo WAV file.
    fn wav(rate: u32, frames: &[[i16; 2]]) -> Vec<u8> {
        let data = frames.len() as u32 * 4;
        let mut out = Vec::new();
        out.extend(b"RIFF");
        out.extend((36 + data).to_le_bytes());
        out.extend(b"WAVEfmt ");
        out.extend(16u32.to_le_bytes());
        out.extend(1u16.to_le_bytes());
        out.extend(2u16.to_le_bytes());
        out.extend(rate.to_le_bytes());
        out.extend((rate * 4).to_le_bytes());
        out.extend(4u16.to_le_bytes());
        out.extend(16u16.to_le_bytes());
        out.extend(b"data");
        out.extend(data.to_le_bytes());
        for f in frames {
            out.extend(f[0].to_le_bytes());
            out.extend(f[1].to_le_bytes());
        }
        out
    }

    #[test]
    fn decodes_and_resamples_to_the_output_rate() {
        // 1.5 s at 44.1 kHz: a click on the left at 1 s, silence on the right.
        let mut frames = vec![[0i16; 2]; 66_150];
        frames[44_100] = [20_000, 0];
        let track = backing_track(wav(44_100, &frames), 48_000).unwrap();
        assert_eq!(track.frames(), 72_000);
        let peak = (0..track.frames() as usize).max_by_key(|&i| track.main[2 * i].unsigned_abs()).unwrap();
        assert!(peak.abs_diff(48_000) <= 1, "the click stays at 1 s: frame {peak}");
        assert!(track.main.iter().skip(1).step_by(2).all(|&r| r == 0));
    }
}
