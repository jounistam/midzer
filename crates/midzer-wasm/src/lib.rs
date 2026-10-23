//! Raw C-ABI exports for the AudioWorklet host.
//!
//! wasm-bindgen is deliberately not used: its JS glue expects APIs that the
//! AudioWorklet global scope lacks, and a handful of plain exports is all the
//! real-time path needs. The worklet reads buffers directly from linear memory
//! via the pointers returned below.

use midzer_core::{Command, Engine, MAX_EVENTS_PER_BLOCK, Track, WORDS_PER_EVENT};
use midzer_synth::Synth;

/// Frames per render quantum in the Web Audio API.
pub const BLOCK_FRAMES: usize = 128;

pub struct Host {
    engine: Engine,
    synth: Synth,
    left: [f32; BLOCK_FRAMES],
    right: [f32; BLOCK_FRAMES],
    /// Microphone input for the next block, filled by the worklet.
    input: [f32; BLOCK_FRAMES],
    /// The live source (a tab or an input) for the next block, filled by the worklet.
    live: [f32; BLOCK_FRAMES],
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
            input: [0.0; BLOCK_FRAMES],
            live: [0.0; BLOCK_FRAMES],
            events: [[0; WORDS_PER_EVENT]; MAX_EVENTS_PER_BLOCK],
        }
    }

    /// Applies an encoded command; returns false if it was invalid.
    pub fn command(&mut self, words: [u32; 4]) -> bool {
        match Command::decode(words) {
            Some(cmd @ Command::SetGain { gain }) => {
                self.synth.set_gain(gain);
                self.engine.handle(cmd);
            }
            Some(Command::SetKitSound { on }) => self.synth.set_kit_sound(on),
            Some(cmd) => self.engine.handle(cmd),
            None => return false,
        }
        true
    }

    /// Feeds `frames` of microphone input from [`Host::input_mut`]; call before [`Host::process`].
    pub fn listen(&mut self, frames: usize) {
        self.engine.listen(&self.input[..frames.min(BLOCK_FRAMES)]);
    }

    pub fn input_mut(&mut self) -> &mut [f32; BLOCK_FRAMES] {
        &mut self.input
    }

    /// Feeds `frames` of the live source from [`Host::live_mut`]; call before [`Host::process`].
    pub fn listen_live(&mut self, frames: usize) {
        self.engine.listen_live(&self.live[..frames.min(BLOCK_FRAMES)]);
    }

    pub fn live_mut(&mut self) -> &mut [f32; BLOCK_FRAMES] {
        &mut self.live
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
        self.engine.mix_track(&mut self.left[..frames], &mut self.right[..frames]);
        self.engine.end_block();
        count
    }

    /// Plays `track` (at this host's rate) in songs while track mode is on; `None` frees it.
    pub fn set_track(&mut self, track: Option<Track>) {
        drop(self.engine.set_track(track.map(Box::new)));
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

/// Allocates `len` 16-bit samples for a recording, see [`mz_load_track`].
#[unsafe(no_mangle)]
pub extern "C" fn mz_alloc_samples(len: usize) -> *mut i16 {
    let mut buf = Vec::<i16>::with_capacity(len);
    let ptr = buf.as_mut_ptr();
    std::mem::forget(buf);
    ptr
}

/// Takes over a recording: `len` interleaved stereo samples at `main`, and
/// as many of the drum stem at `drums` unless it is null. Both come from
/// [`mz_alloc_samples`] and must not be used afterwards. A zero `len` unloads it.
///
/// # Safety
/// `host` must be live; the pointers must come from [`mz_alloc_samples`] with `len`, filled by the caller.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mz_load_track(host: *mut Host, main: *mut i16, drums: *mut i16, len: usize) {
    let take = |ptr: *mut i16| (!ptr.is_null()).then(|| unsafe { Vec::from_raw_parts(ptr, len, len) });
    let track = (len > 0).then(|| Track { main: take(main).unwrap_or_default(), drums: take(drums) });
    unsafe { (*host).set_track(track) }
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

/// Where the worklet writes a block of microphone input.
///
/// # Safety
/// `host` must be a live pointer from [`mz_create`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mz_input_ptr(host: *mut Host) -> *mut f32 {
    unsafe { (*host).input.as_mut_ptr() }
}

/// Analyses `frames` of microphone input from [`mz_input_ptr`]; call before [`mz_process`].
///
/// # Safety
/// `host` must be a live pointer from [`mz_create`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mz_listen(host: *mut Host, frames: u32) {
    unsafe { (*host).listen(frames as usize) }
}

/// Where the worklet writes a block of the live source.
///
/// # Safety
/// `host` must be a live pointer from [`mz_create`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mz_live_ptr(host: *mut Host) -> *mut f32 {
    unsafe { (*host).live.as_mut_ptr() }
}

/// The engine frame of the next block's first sample (for the listener's input, see `midzer-hear`).
///
/// # Safety
/// `host` must be a live pointer from [`mz_create`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mz_now(host: *const Host) -> f64 {
    unsafe { (*host).engine.now() as f64 }
}

/// Tracks the beat of `frames` of the live source from [`mz_live_ptr`]; call before [`mz_process`].
///
/// # Safety
/// `host` must be a live pointer from [`mz_create`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mz_listen_live(host: *mut Host, frames: u32) {
    unsafe { (*host).listen_live(frames as usize) }
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
