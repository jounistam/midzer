//! Raw C-ABI exports for the web app's listening worker (no wasm-bindgen,
//! like `midzer-wasm`). The worker creates a [`Hearing`] with the input's
//! sample rate, copies input into [`mzh_input_ptr`] and pushes it with the
//! engine frame of its first sample, then steps it; each result is a list of
//! f64s at [`mzh_result_ptr`]:
//!
//! - a grid (step returns 1): the beat's engine frame, the period in frames,
//!   which beat of the bar it is (−1 unknown) and the confidence;
//! - chords (step returns 2): per span from, to (engine frames), the chord
//!   as root · 16 + the engine's quality index (−1 for none), and 1 if it is
//!   predicted from the song's loop rather than heard (else 0);
//! - a song followed (step returns 3): its id (−1 when none is any more),
//!   the engine frame, the seconds into the song there, and the votes;
//! - its bar lines (step returns 4): per bar the engine frame of the beat it
//!   starts on and its length in beats.
//!
//! With capture on, each song heard (see [`Hearing::set_capture`]) waits in
//! [`mzh_take_song`] as 16-bit samples at 22.05 kHz.
//!
//! The songs to recognise go in with [`mzh_library_add`], from buffers the
//! worker gets with [`mzh_alloc`] and gives back with [`mzh_free`].

use midzer_core::Quality;

use crate::recognise::chord_of_code;
use crate::{Capture, Heard, Hearing, StoredSong};

/// The most input one push takes.
pub const INPUT: usize = 16_384;

pub struct Listener {
    hearing: Hearing,
    input: Vec<f32>,
    result: Vec<f64>,
    song: Capture,
}

/// A listener for input at `rate` Hz, or null if the networks failed to load.
#[unsafe(no_mangle)]
pub extern "C" fn mzh_create(rate: f32) -> *mut Listener {
    match Hearing::with_models(rate) {
        Ok(hearing) => Box::into_raw(Box::new(Listener { hearing, input: vec![0.0; INPUT], result: Vec::new(), song: Capture::default() })),
        Err(_) => std::ptr::null_mut(),
    }
}

/// # Safety
/// `l` must come from [`mzh_create`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mzh_input_ptr(l: *mut Listener) -> *mut f32 {
    unsafe { (*l).input.as_mut_ptr() }
}

/// Takes `len` samples (at most [`INPUT`]) from [`mzh_input_ptr`]; the first
/// is engine frame `frame`.
///
/// # Safety
/// `l` must come from [`mzh_create`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mzh_push(l: *mut Listener, len: u32, frame: f64) {
    let l = unsafe { &mut *l };
    let len = (len as usize).min(INPUT);
    l.hearing.push(&l.input[..len], frame as u64);
}

/// Runs a network if one is due: 0 nothing, 1 a grid, 2 chords, 3 a song
/// followed, 4 its bar lines (see the module docs).
///
/// # Safety
/// `l` must come from [`mzh_create`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mzh_step(l: *mut Listener) -> u32 {
    let l = unsafe { &mut *l };
    l.result.clear();
    match l.hearing.step() {
        None => 0,
        Some(Heard::Grid(g)) => {
            l.result.extend([g.beat, g.period, g.one.map_or(-1.0, |o| o as f64), g.confidence as f64]);
            1
        }
        Some(Heard::Follow(f)) => {
            l.result.extend(f.map_or([-1.0, 0.0, 0.0, 0.0], |f| [f.song as f64, f.frame, f.seconds, f.votes as f64]));
            3
        }
        Some(Heard::Bars(bars)) => {
            l.result.extend(bars.iter().flat_map(|&(frame, beats)| [frame, beats as f64]));
            4
        }
        Some(Heard::Chords(spans)) => {
            for s in spans {
                let code = s.chord.map_or(-1.0, |c| (c.root as usize * 16 + Quality::ALL.iter().position(|&q| q == c.quality).unwrap_or(0)) as f64);
                l.result.extend([s.from, s.to, code, s.predicted as u8 as f64]);
            }
            2
        }
    }
}

/// # Safety
/// `l` must come from [`mzh_create`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mzh_result_ptr(l: *const Listener) -> *const f64 {
    unsafe { (*l).result.as_ptr() }
}

/// # Safety
/// `l` must come from [`mzh_create`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mzh_result_len(l: *const Listener) -> u32 {
    unsafe { (*l).result.len() as u32 }
}

/// Forgets the song.
///
/// # Safety
/// `l` must come from [`mzh_create`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mzh_reset(l: *mut Listener) {
    unsafe { (*l).hearing.reset() }
}

/// Captures songs, or stops (1 or 0).
///
/// # Safety
/// `l` must come from [`mzh_create`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mzh_set_capture(l: *mut Listener, on: u32) {
    unsafe { (*l).hearing.set_capture(on != 0) }
}

/// Ends the song being captured here.
///
/// # Safety
/// `l` must come from [`mzh_create`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mzh_finish(l: *mut Listener) {
    unsafe { (*l).hearing.finish() }
}

/// Takes the last song captured: its length in samples (0 if none), read
/// from [`mzh_song_ptr`] until the next call.
///
/// # Safety
/// `l` must come from [`mzh_create`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mzh_take_song(l: *mut Listener) -> u32 {
    let l = unsafe { &mut *l };
    l.song = l.hearing.take_song().unwrap_or_default();
    l.song.samples.len() as u32
}

/// # Safety
/// `l` must come from [`mzh_create`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mzh_song_ptr(l: *const Listener) -> *const i16 {
    unsafe { (*l).song.samples.as_ptr() }
}

/// Bit 0: heard from its start; bit 1: heard to its end.
///
/// # Safety
/// `l` must come from [`mzh_create`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mzh_song_flags(l: *const Listener) -> u32 {
    let s = unsafe { &(*l).song };
    s.clean_start as u32 | (s.clean_end as u32) << 1
}

/// The id of the stored song followed for most of the last capture taken, or −1.
///
/// # Safety
/// `l` must come from [`mzh_create`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mzh_song_followed(l: *const Listener) -> i32 {
    unsafe { (*l).song.followed.map_or(-1, |id| id as i32) }
}

/// `bytes` of room, 8-aligned, for [`mzh_library_add`].
#[unsafe(no_mangle)]
pub extern "C" fn mzh_alloc(bytes: usize) -> *mut u8 {
    let mut buf = Vec::<u64>::with_capacity(bytes.div_ceil(8));
    let ptr = buf.as_mut_ptr();
    std::mem::forget(buf);
    ptr as *mut u8
}

/// # Safety
/// `ptr` must come from [`mzh_alloc`] with the same `bytes`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mzh_free(ptr: *mut u8, bytes: usize) {
    unsafe { drop(Vec::<u64>::from_raw_parts(ptr as *mut u64, 0, bytes.div_ceil(8))) }
}

/// Keeps a song to recognise, replacing the one with its id: `pairs`
/// landmarks (hash, frame), `n_beats` beat times in seconds (the chart's,
/// plus its end), a chord code per beat (root · 16 + the quality index, −1
/// none), the length of the audio the landmarks came from, and the beats of
/// each bar (`n_bars` of them; none means bars of four).
///
/// # Safety
/// `l` must come from [`mzh_create`]; the buffers from [`mzh_alloc`], holding as many as said.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mzh_library_add(l: *mut Listener, id: u32, marks: *const u32, pairs: u32, beats: *const f64, n_beats: u32, chords: *const i32, n_chords: u32, seconds: f64, bar_beats: *const u8, n_bars: u32) {
    let l = unsafe { &mut *l };
    let (marks, beats, chords) = unsafe {
        (std::slice::from_raw_parts(marks, pairs as usize * 2), std::slice::from_raw_parts(beats, n_beats as usize), std::slice::from_raw_parts(chords, n_chords as usize))
    };
    let bar_beats = if n_bars == 0 { Vec::new() } else { unsafe { std::slice::from_raw_parts(bar_beats, n_bars as usize) }.to_vec() };
    let chords = chords.iter().map(|&c| chord_of_code(c)).collect();
    let song = StoredSong { id, beats: beats.to_vec(), chords, seconds, bar_beats };
    l.hearing.library().add(song, marks.as_chunks::<2>().0.iter().map(|p| (p[0], p[1])).collect());
}

/// # Safety
/// `l` must come from [`mzh_create`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mzh_library_remove(l: *mut Listener, id: u32) {
    unsafe { (*l).hearing.library().remove(id) }
}

/// # Safety
/// `l` must come from [`mzh_create`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mzh_library_clear(l: *mut Listener) {
    unsafe { (*l).hearing.library().clear() }
}

/// "Not this song": stops following song `id` until the next silence or reset.
///
/// # Safety
/// `l` must come from [`mzh_create`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mzh_ignore(l: *mut Listener, id: u32) {
    unsafe { (*l).hearing.ignore(id) }
}
