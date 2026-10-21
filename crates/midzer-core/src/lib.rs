//! Platform-independent midzer engine.
//!
//! Everything here must be safe to call from a real-time audio callback:
//! no I/O, no locks, and no allocation after construction.

mod band;
mod chord;
mod command;
mod conductor;
mod drummer;
mod drums;
mod engine;
mod event;
mod listen;
mod song;
pub mod style;
mod voicing;

pub use band::BandDrums;
pub use chord::{Chord, PitchSet, Quality, recognize as recognize_chord};
pub use command::{Command, WORDS_PER_COMMAND};
pub use conductor::{IdleMode, Request, Section};
pub use drummer::COUNT_IN_HITS;
pub use drums::{Instrument, Kit, KitAction, Piece, Trigger};
pub use song::{MAX_SONG_BARS, Slot};
pub use engine::{Engine, PLAYER_CHANNEL};
pub use event::{BandState, DRUM_CHANNEL, MIC_CHANNEL, PLAYER_DRUM_CHANNEL, EngineEvent, EventBuffer, MAX_EVENTS_PER_BLOCK, Timed, WORDS_PER_EVENT};
pub use listen::{Heard, HeardList, ListenMode, Listener};
