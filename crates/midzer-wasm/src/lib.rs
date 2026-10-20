//! Raw C-ABI exports for the AudioWorklet host.
//!
//! wasm-bindgen is deliberately not used: its JS glue expects APIs that the
//! AudioWorklet global scope lacks, and a handful of plain exports is all the
//! real-time path needs. The worklet reads buffers directly from linear memory
//! via the pointers returned below.

use midzer_core::{Command, Engine, MAX_EVENTS_PER_BLOCK, WORDS_PER_EVENT};
use midzer_synth::Synth;

/// Frames per render quantum in the Web Audio API.
pub const BLOCK_FRAMES: usize = 128;

pub struct Host {
    engine: Engine,
    synth: Synth,
    left: [f32; BLOCK_FRAMES],
    right: [f32; BLOCK_FRAMES],
    /// Events of the last block, encoded for the main thread.
    events: [[u32; WORDS_PER_EVENT]; MAX_EVENTS_PER_BLOCK],
}

impl Host {
    pub fn new(sample_rate: f32) -> Self {
        Self {
            engine: Engine::new(sample_rate),
            synth: Synth::new(sample_rate),
            left: [0.0; BLOCK_FRAMES],
            right: [0.0; BLOCK_FRAMES],
            events: [[0; WORDS_PER_EVENT]; MAX_EVENTS_PER_BLOCK],
        }
    }

    /// Applies an encoded command; returns false if it was invalid.
    pub fn command(&mut self, words: [u32; 4]) -> bool {
        match Command::decode(words) {
            Some(Command::SetGain { gain }) => self.synth.set_gain(gain),
            Some(cmd) => self.engine.handle(cmd),
            None => return false,
        }
        true
    }

    /// Renders up to [`BLOCK_FRAMES`] frames; returns the number of encoded events.
    pub fn process(&mut self, frames: usize) -> usize {
        let frames = frames.min(BLOCK_FRAMES);
        let events = self.engine.process(frames as u32);
        self.synth.render(events, &mut self.left[..frames], &mut self.right[..frames]);
        for (slot, timed) in self.events.iter_mut().zip(events) {
            *slot = timed.event.encode(timed.frame);
        }
        let count = events.len();
        self.engine.end_block();
        count
    }

    pub fn load_soundfont(&mut self, bytes: &[u8]) -> bool {
        self.synth.load_soundfont(bytes).is_ok()
    }

    pub fn output(&self) -> (&[f32], &[f32]) {
        (&self.left, &self.right)
    }

    pub fn events(&self, count: usize) -> &[[u32; WORDS_PER_EVENT]] {
        &self.events[..count]
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn mz_create(sample_rate: f32) -> *mut Host {
    Box::into_raw(Box::new(Host::new(sample_rate)))
}

/// # Safety
/// `host` must come from [`mz_create`] and must not be used afterwards.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mz_destroy(host: *mut Host) {
    if !host.is_null() {
        drop(unsafe { Box::from_raw(host) });
    }
}

/// Allocates `len` bytes for the caller to fill, e.g. with a SoundFont.
#[unsafe(no_mangle)]
pub extern "C" fn mz_alloc(len: usize) -> *mut u8 {
    let mut buf = Vec::<u8>::with_capacity(len);
    let ptr = buf.as_mut_ptr();
    std::mem::forget(buf);
    ptr
}

/// # Safety
/// `ptr` and `len` must come from one [`mz_alloc`] call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mz_free(ptr: *mut u8, len: usize) {
    drop(unsafe { Vec::from_raw_parts(ptr, 0, len) });
}

/// Parses a SoundFont from `len` bytes at `ptr`. Returns 1 on success.
///
/// # Safety
/// `host` must be live; `ptr..ptr+len` must be initialised memory from [`mz_alloc`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mz_load_soundfont(host: *mut Host, ptr: *const u8, len: usize) -> u32 {
    let bytes = unsafe { std::slice::from_raw_parts(ptr, len) };
    unsafe { (*host).load_soundfont(bytes) as u32 }
}

/// # Safety
/// `host` must be a live pointer from [`mz_create`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mz_left_ptr(host: *const Host) -> *const f32 {
    unsafe { (*host).left.as_ptr() }
}

/// # Safety
/// `host` must be a live pointer from [`mz_create`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mz_right_ptr(host: *const Host) -> *const f32 {
    unsafe { (*host).right.as_ptr() }
}

/// Encoded events of the last [`mz_process`] call, `WORDS_PER_EVENT` u32 each.
///
/// # Safety
/// `host` must be a live pointer from [`mz_create`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mz_events_ptr(host: *const Host) -> *const u32 {
    unsafe { (*host).events.as_ptr().cast() }
}

/// Applies one encoded command. Returns 1 if it was valid, 0 if it was ignored.
///
/// # Safety
/// `host` must be a live pointer from [`mz_create`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mz_command(host: *mut Host, w0: u32, w1: u32, w2: u32, w3: u32) -> u32 {
    unsafe { (*host).command([w0, w1, w2, w3]) as u32 }
}

/// Renders `frames` (at most [`BLOCK_FRAMES`]) into the output buffers and
/// returns how many events [`mz_events_ptr`] now holds.
///
/// # Safety
/// `host` must be a live pointer from [`mz_create`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mz_process(host: *mut Host, frames: u32) -> u32 {
    unsafe { (*host).process(frames as usize) as u32 }
}
