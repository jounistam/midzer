//! midzer's weight files (`.mzm`), written by `scripts/models/export.py`:
//! b"MZM1", a u32 tensor count, then per tensor a u16 name length, the name,
//! a u8 dtype (0 = f16, 1 = f32), a u8 rank, u32 dims and the data, all
//! little-endian.

use std::collections::HashMap;

pub struct Tensor {
    pub shape: Vec<usize>,
    pub data: Vec<f32>,
}

pub struct Weights {
    tensors: HashMap<String, Tensor>,
}

impl Weights {
    pub fn parse(bytes: &[u8]) -> Result<Self, String> {
        let mut at = 0usize;
        let mut take = |n: usize| -> Result<&[u8], String> {
            let s = bytes.get(at..at + n).ok_or("The weight file is cut short.")?;
            at += n;
            Ok(s)
        };
        if take(4)? != b"MZM1" {
            return Err("Not a midzer weight file.".into());
        }
        let u32_at = |b: &[u8]| u32::from_le_bytes([b[0], b[1], b[2], b[3]]);
        let count = u32_at(take(4)?) as usize;
        let mut tensors = HashMap::with_capacity(count);
        for _ in 0..count {
            let len = u16::from_le_bytes(take(2)?.try_into().unwrap()) as usize;
            let name = String::from_utf8(take(len)?.to_vec()).map_err(|_| "A tensor name isn't UTF-8.")?;
            let head = take(2)?;
            let (dtype, rank) = (head[0], head[1] as usize);
            let mut shape = Vec::with_capacity(rank);
            for _ in 0..rank {
                shape.push(u32_at(take(4)?) as usize);
            }
            let n: usize = shape.iter().product();
            let data = match dtype {
                0 => take(2 * n)?.as_chunks::<2>().0.iter().map(|&c| f16_to_f32(u16::from_le_bytes(c))).collect(),
                1 => take(4 * n)?.as_chunks::<4>().0.iter().map(|&c| f32::from_le_bytes(c)).collect(),
                _ => return Err(format!("Tensor {name} has an unknown type.")),
            };
            tensors.insert(name, Tensor { shape, data });
        }
        Ok(Self { tensors })
    }

    /// Takes a tensor out, checking its shape.
    pub fn take(&mut self, name: &str, shape: &[usize]) -> Result<Vec<f32>, String> {
        let t = self.tensors.remove(name).ok_or_else(|| format!("The weights have no {name}."))?;
        if t.shape != shape {
            return Err(format!("{name} is {:?}, expected {shape:?}.", t.shape));
        }
        Ok(t.data)
    }

    pub fn get(&self, name: &str) -> Option<&Tensor> {
        self.tensors.get(name)
    }
}

pub fn f16_to_f32(h: u16) -> f32 {
    let sign = ((h >> 15) as u32) << 31;
    let exp = ((h >> 10) & 0x1f) as u32;
    let frac = (h & 0x3ff) as u32;
    let bits = match exp {
        0 if frac == 0 => sign,
        0 => {
            // Subnormal: normalise it.
            let mut e = 127 - 15 + 1;
            let mut f = frac;
            while f & 0x400 == 0 {
                f <<= 1;
                e -= 1;
            }
            sign | (e << 23) | ((f & 0x3ff) << 13)
        }
        31 => sign | 0x7f80_0000 | (frac << 13),
        _ => sign | ((exp + 127 - 15) << 23) | (frac << 13),
    };
    f32::from_bits(bits)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn half_floats() {
        assert_eq!(f16_to_f32(0x3c00), 1.0);
        assert_eq!(f16_to_f32(0xc000), -2.0);
        assert_eq!(f16_to_f32(0x0000), 0.0);
        assert!((f16_to_f32(0x0001) - 5.960_464_5e-8).abs() < 1e-12);
        assert!((f16_to_f32(0x3555) - 0.333_251_95).abs() < 1e-7);
    }
}
