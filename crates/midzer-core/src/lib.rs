//! Platform-independent midzer engine.
//!
//! Everything here must be safe to call from a real-time audio callback:
//! no I/O, no locks, and no allocation after construction.

mod band;
mod chord;
mod command;
mod conductor;
mod engine;
mod event;
mod song;
pub mod style;
mod voicing;

pub use chord::{Chord, PitchSet, Quality, recognize as recognize_chord};
pub use command::{Command, WORDS_PER_COMMAND};
pub use conductor::{IdleMode, Request, Section};
pub use song::{MAX_SONG_BARS, Slot};
pub use engine::{Engine, PLAYER_CHANNEL};
pub use event::{BandState, DRUM_CHANNEL, EngineEvent, EventBuffer, MAX_EVENTS_PER_BLOCK, Timed, WORDS_PER_EVENT};
