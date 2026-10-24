//! Hears a song as it plays, for playing along with it: where the beats and
//! the 1s are, and the chords. Two small networks do the listening, ported
//! to plain Rust so they run the same in the browser (wasm, in a worker) and
//! in the desktop app, off the audio thread:
//!
//! - Beat This! small (Foscarin, Schlüter, Widmer, ISMIR 2024; MIT): beats
//!   and downbeats from a log-mel spectrogram.
//! - BTC (Park et al., ISMIR 2019; MIT): chords from a constant-Q spectrum.
//!
//! The weights are converted by `scripts/models/export.py` and kept in
//! `models/` as f16.

// The networks index several parallel buffers at once.
#![allow(clippy::needless_range_loop, clippy::too_many_arguments)]

pub mod beat_this;
pub mod btc;
pub mod cqt;
pub mod fingerprint;
#[cfg(target_arch = "wasm32")]
mod ffi;
pub mod hearing;
pub mod mel;
pub mod mzm;
pub mod nn;
pub mod offline;
pub mod resample;

pub use beat_this::BeatThis;
pub use btc::Btc;
pub use fingerprint::{Landmark, fingerprint};
pub use hearing::{Capture, ChordSpan, Grid, Heard, Hearing, Settings, chord_over};
pub use offline::{SongHeard, TimedChord, hear_beats, hear_chords, hear_song};

/// The weights, as converted.
pub const BEAT_THIS: &[u8] = include_bytes!("../models/beat-this-small.mzm");
pub const BTC: &[u8] = include_bytes!("../models/btc-large.mzm");
