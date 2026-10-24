//! Raw C-ABI exports for the web app's analysis worker (no wasm-bindgen,
//! like `midzer-wasm`). The worker copies the file into memory from
//! [`mza_alloc`], calls [`mza_analyze`] and reads the JSON result; progress
//! arrives through the imported `env.mza_progress`.

use std::sync::Mutex;

use crate::{Step, analyze_file, analyze_heard, backing_track};

#[link(wasm_import_module = "env")]
unsafe extern "C" {
    fn mza_progress(fraction: f32, step: u32);
}

static RESULT: Mutex<Vec<u8>> = Mutex::new(Vec::new());
static SAMPLES: Mutex<Vec<i16>> = Mutex::new(Vec::new());

/// Allocates `len` bytes for the file; [`mza_analyze`] takes them over.
#[unsafe(no_mangle)]
pub extern "C" fn mza_alloc(len: usize) -> *mut u8 {
    let mut buf = Vec::<u8>::with_capacity(len);
    let ptr = buf.as_mut_ptr();
    std::mem::forget(buf);
    ptr
}

/// Analyses the `len` bytes at `ptr`. Returns 1 when the result is the
/// analysis as JSON, 0 when it is an error message.
///
/// # Safety
/// `ptr` must come from [`mza_alloc`] with the same `len`, filled by the caller.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mza_analyze(ptr: *mut u8, len: usize) -> u32 {
    let bytes = unsafe { Vec::from_raw_parts(ptr, len, len) };
    let mut progress = |f: f32, step: Step| unsafe { mza_progress(f, step as u32) };
    let (ok, out) = match analyze_file(bytes, &mut progress) {
        Ok(a) => (1, serde_json::to_vec(&a).unwrap_or_default()),
        Err(e) => (0, e.into_bytes()),
    };
    *RESULT.lock().unwrap_or_else(|e| e.into_inner()) = out;
    ok
}

/// Analyses a song heard live: the `len` bytes at `ptr` are 16-bit mono
/// samples at 22.05 kHz. Returns like [`mza_analyze`].
///
/// # Safety
/// `ptr` must come from [`mza_alloc`] with the same `len`, filled by the caller.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mza_analyze_heard(ptr: *mut u8, len: usize) -> u32 {
    let bytes = unsafe { Vec::from_raw_parts(ptr, len, len) };
    let samples: Vec<i16> = bytes.chunks_exact(2).map(|b| i16::from_le_bytes([b[0], b[1]])).collect();
    drop(bytes);
    let mut progress = |f: f32, step: Step| unsafe { mza_progress(f, step as u32) };
    let (ok, out) = match analyze_heard(&samples, &mut progress) {
        Ok(a) => (1, serde_json::to_vec(&a).unwrap_or_default()),
        Err(e) => (0, e.into_bytes()),
    };
    *RESULT.lock().unwrap_or_else(|e| e.into_inner()) = out;
    ok
}

#[unsafe(no_mangle)]
pub extern "C" fn mza_result_ptr() -> *const u8 {
    RESULT.lock().unwrap_or_else(|e| e.into_inner()).as_ptr()
}

#[unsafe(no_mangle)]
pub extern "C" fn mza_result_len() -> usize {
    RESULT.lock().unwrap_or_else(|e| e.into_inner()).len()
}

/// Decodes the `len` bytes at `ptr` (from [`mza_alloc`]) as a recording to
/// play along with, at `rate`. Returns 1 when [`mza_samples_ptr`] holds it as
/// interleaved stereo 16-bit samples, 0 when the result is an error message.
///
/// # Safety
/// `ptr` must come from [`mza_alloc`] with the same `len`, filled by the caller.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mza_track(ptr: *mut u8, len: usize, rate: u32) -> u32 {
    let bytes = unsafe { Vec::from_raw_parts(ptr, len, len) };
    let (ok, samples, message) = match backing_track(bytes, rate) {
        Ok(t) => (1, t.main, Vec::new()),
        Err(e) => (0, Vec::new(), e.into_bytes()),
    };
    *SAMPLES.lock().unwrap_or_else(|e| e.into_inner()) = samples;
    *RESULT.lock().unwrap_or_else(|e| e.into_inner()) = message;
    ok
}

#[unsafe(no_mangle)]
pub extern "C" fn mza_samples_ptr() -> *const i16 {
    SAMPLES.lock().unwrap_or_else(|e| e.into_inner()).as_ptr()
}

#[unsafe(no_mangle)]
pub extern "C" fn mza_samples_len() -> usize {
    SAMPLES.lock().unwrap_or_else(|e| e.into_inner()).len()
}
