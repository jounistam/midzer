//! 16-bit stereo WAV files for the separated stems.

use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::Path;

pub fn write(path: &Path, rate: u32, channels: &[Vec<f32>; 2]) -> std::io::Result<()> {
    let frames = channels[0].len() as u32;
    let data = frames * 4;
    let mut out = BufWriter::new(File::create(path)?);
    out.write_all(b"RIFF")?;
    out.write_all(&(36 + data).to_le_bytes())?;
    out.write_all(b"WAVEfmt ")?;
    out.write_all(&16u32.to_le_bytes())?;
    out.write_all(&1u16.to_le_bytes())?; // PCM
    out.write_all(&2u16.to_le_bytes())?;
    out.write_all(&rate.to_le_bytes())?;
    out.write_all(&(rate * 4).to_le_bytes())?;
    out.write_all(&4u16.to_le_bytes())?;
    out.write_all(&16u16.to_le_bytes())?;
    out.write_all(b"data")?;
    out.write_all(&data.to_le_bytes())?;
    for i in 0..frames as usize {
        for c in channels {
            out.write_all(&((c[i].clamp(-1.0, 1.0) * 32767.0) as i16).to_le_bytes())?;
        }
    }
    out.flush()
}
