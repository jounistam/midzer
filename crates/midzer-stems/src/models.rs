//! The ONNX models, downloaded once into the app's data folder and checked by hash.

use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

pub struct Model {
    pub file: &'static str,
    pub url: &'static str,
    pub sha256: &'static str,
    pub bytes: u64,
}

/// HT-Demucs v4 (MIT), exported to a single ONNX graph with fp16 weights.
pub const DEMUCS: Model = Model {
    file: "htdemucs_fp16weights.onnx",
    url: "https://huggingface.co/adowu/htdemucs-onnx/resolve/e6b688a9a9f977e2efaba79d0f18b9b0cfc606ea/htdemucs_fp16weights.onnx",
    sha256: "d05c269d0178d2a72ad484b10b11dd370193fc923201c3b27a99f848745db70a",
    bytes: 165_612_636,
};

/// Spotify's basic-pitch (Apache-2.0), the ICASSP 2022 model.
pub const BASIC_PITCH: Model = Model {
    file: "basic-pitch-nmp.onnx",
    url: "https://raw.githubusercontent.com/spotify/basic-pitch/fa5997af0a8210982619003269994a1be25eddf3/basic_pitch/saved_models/icassp_2022/nmp.onnx",
    sha256: "2c3c1d144bfa61ad236e92e169c13535c880469a12a047d4e73451f2c059a0ec",
    bytes: 230_444,
};

pub const ALL: [&Model; 2] = [&DEMUCS, &BASIC_PITCH];

/// Total download size of the models not yet in `dir`.
pub fn missing_bytes(dir: &Path) -> u64 {
    ALL.iter().filter(|m| !dir.join(m.file).exists()).map(|m| m.bytes).sum()
}

/// The model's path in `dir`, downloading it first if needed. `progress` gets bytes done.
pub fn ensure(dir: &Path, model: &Model, progress: &mut dyn FnMut(u64)) -> Result<PathBuf, String> {
    let path = dir.join(model.file);
    if path.exists() {
        return Ok(path);
    }
    fs::create_dir_all(dir).map_err(|e| format!("Could not create {}: {e}", dir.display()))?;
    let partial = dir.join(format!("{}.part", model.file));
    let response = ureq::get(model.url).call().map_err(|e| format!("Could not download {}: {e}", model.file))?;
    let mut reader = response.into_body().into_reader();
    let mut file = File::create(&partial).map_err(|e| e.to_string())?;
    let mut hash = Sha256::new();
    let mut buf = vec![0u8; 1 << 16];
    let mut done = 0u64;
    loop {
        let n = reader.read(&mut buf).map_err(|e| format!("The download of {} broke off: {e}", model.file))?;
        if n == 0 {
            break;
        }
        hash.update(&buf[..n]);
        file.write_all(&buf[..n]).map_err(|e| e.to_string())?;
        done += n as u64;
        progress(done);
    }
    drop(file);
    let digest: String = hash.finalize().iter().map(|b| format!("{b:02x}")).collect();
    if digest != model.sha256 {
        let _ = fs::remove_file(&partial);
        return Err(format!("The download of {} was damaged (its checksum doesn't match).", model.file));
    }
    fs::rename(&partial, &path).map_err(|e| e.to_string())?;
    Ok(path)
}
