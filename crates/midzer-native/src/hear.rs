//! The listening thread: runs `midzer-hear` over the live source while it is
//! followed. The audio callback hands it blocks tagged with their engine
//! frames; its beat grids go to the engine as `LiveGuide` commands and its
//! chords to the forwarder, for the UI. Each song it captures waits in
//! `Shared::live_song` to be analysed, and the forwarder tells the UI. The
//! stored songs it recognises come from the UI through `Shared::library`.

use std::sync::atomic::Ordering;
use std::sync::{Arc, mpsc};
use std::time::Duration;

use midzer_core::{Command, Quality};
use midzer_hear::recognise::chord_of_code;
use midzer_hear::{Capture, Heard, Hearing, Library, StoredSong};

use crate::Shared;

const BLOCK: usize = 128;
/// About 11 s of 48 kHz audio: room for a network run.
pub(crate) const TAP_BLOCKS: usize = 4096;
const POLL: Duration = Duration::from_millis(25);

/// One block of the live source and the engine frame of its first sample.
#[derive(Clone, Copy)]
pub(crate) struct TapBlock {
    frame: u64,
    len: u16,
    samples: [f32; BLOCK],
}

impl TapBlock {
    pub(crate) fn new(frame: u64, input: &[f32]) -> Self {
        let mut samples = [0.0; BLOCK];
        let len = input.len().min(BLOCK);
        samples[..len].copy_from_slice(&input[..len]);
        Self { frame, len: len as u16, samples }
    }
}

/// What the listener tells the UI, through the forwarder.
pub(crate) enum Listened {
    /// Chord spans, as in `Update::Chords`.
    Chords(Vec<f64>),
    /// A song captured, now in `Shared::live_song`, and the stored song followed in it (−1 none).
    Song { seconds: f64, flags: u32, followed: i32 },
    /// A stored song followed, as in `Update::LiveFollow`.
    Follow { id: i32, frame: f64, seconds: f64 },
}

/// The stored songs as `encodeLibrary` in native.ts lays them out.
pub(crate) fn decode_library(bytes: &[u8]) -> Result<Library, String> {
    let mut rest = bytes;
    let mut take = |n: usize| -> Result<&[u8], String> {
        if rest.len() < n {
            return Err("the library ends early".into());
        }
        let (head, tail) = rest.split_at(n);
        rest = tail;
        Ok(head)
    };
    let mut library = Library::default();
    let word = |b: &[u8]| u32::from_le_bytes(b.try_into().unwrap());
    let songs = word(take(4)?);
    for _ in 0..songs {
        let head = take(24)?;
        let (id, pairs, beats, chords) = (word(&head[..4]), word(&head[4..8]) as usize, word(&head[8..12]) as usize, word(&head[12..16]) as usize);
        let seconds = f64::from_le_bytes(head[16..].try_into().unwrap());
        let marks = take(pairs * 8)?.as_chunks::<8>().0.iter().map(|p| (word(&p[..4]), word(&p[4..]))).collect();
        let beats = take(beats * 8)?.as_chunks::<8>().0.iter().map(|b| f64::from_le_bytes(*b)).collect();
        let chords = take(chords * 4)?.as_chunks::<4>().0.iter().map(|c| chord_of_code(i32::from_le_bytes(*c))).collect();
        library.add(StoredSong { id, beats, chords, seconds }, marks);
    }
    Ok(library)
}

fn offer(shared: &Shared, out: &mpsc::Sender<Listened>, song: Capture) {
    let (seconds, flags) = (song.seconds(), song.clean_start as u32 | (song.clean_end as u32) << 1);
    let followed = song.followed.map_or(-1, |id| id as i32);
    if let Ok(mut slot) = shared.live_song.lock() {
        *slot = Some(song);
        let _ = out.send(Listened::Song { seconds, flags, followed });
    }
}

pub(crate) fn listen(shared: Arc<Shared>, out: mpsc::Sender<Listened>) {
    let mut hearing: Option<Hearing> = None;
    let mut generation = shared.live_generation.load(Ordering::Acquire);
    let mut run: Vec<f32> = Vec::new();
    let mut run_start = 0u64;
    // The stored songs, until the listener loads.
    let mut library: Option<Library> = None;
    while shared.running.load(Ordering::Acquire) {
        if let Some(new) = shared.library.lock().ok().and_then(|mut l| l.take()) {
            match hearing.as_mut() {
                Some(h) => *h.library() = new,
                None => library = Some(new),
            }
        }
        while let Some(id) = shared.ignore.pop() {
            if let Some(h) = hearing.as_mut() {
                h.ignore(id);
            }
        }
        // Before a reset: closing the source ends the song first.
        if shared.song_finish.swap(false, Ordering::AcqRel)
            && let Some(h) = hearing.as_mut()
        {
            while let Some(block) = shared.tap.pop() {
                h.push(&block.samples[..block.len as usize], block.frame);
            }
            h.finish();
            if let Some(song) = h.take_song() {
                offer(&shared, &out, song);
            }
        }
        let now = shared.live_generation.load(Ordering::Acquire);
        if now != generation {
            generation = now;
            if let Some(h) = hearing.as_mut() {
                h.reset();
            }
        }
        if !shared.live_open.load(Ordering::Acquire) {
            while shared.tap.pop().is_some() {}
            std::thread::sleep(POLL);
            continue;
        }
        // The networks load once, when first needed.
        let h = match hearing.as_mut() {
            Some(h) => h,
            None => match Hearing::with_models(shared.sample_rate as f32) {
                Ok(mut h) => {
                    h.set_capture(true);
                    if let Some(l) = library.take() {
                        *h.library() = l;
                    }
                    hearing.insert(h)
                }
                Err(e) => {
                    eprintln!("midzer: the listener did not load: {e}");
                    return;
                }
            },
        };
        while let Some(block) = shared.tap.pop() {
            if !run.is_empty() && block.frame != run_start + run.len() as u64 {
                h.push(&run, run_start);
                run.clear();
            }
            if run.is_empty() {
                run_start = block.frame;
            }
            run.extend_from_slice(&block.samples[..block.len as usize]);
        }
        if !run.is_empty() {
            h.push(&run, run_start);
            run.clear();
        }
        match h.step() {
            Some(Heard::Grid(g)) => {
                let cmd = Command::LiveGuide { frame: g.beat.round() as u64 as u32, period: g.period as f32, one: g.one, confidence: (g.confidence * 255.0) as u8 };
                let _ = shared.commands.push(cmd.encode());
            }
            Some(Heard::Chords(spans)) => {
                let flat = spans
                    .iter()
                    .flat_map(|s| {
                        let code = s.chord.map_or(-1.0, |c| (c.root as usize * 16 + Quality::ALL.iter().position(|&q| q == c.quality).unwrap_or(0)) as f64);
                        [s.from, s.to, code, s.predicted as u8 as f64]
                    })
                    .collect();
                let _ = out.send(Listened::Chords(flat));
            }
            Some(Heard::Follow(f)) => {
                let _ = out.send(f.map_or(Listened::Follow { id: -1, frame: 0.0, seconds: 0.0 }, |f| Listened::Follow { id: f.song as i32, frame: f.frame, seconds: f.seconds }));
            }
            None => std::thread::sleep(POLL),
        }
        if let Some(song) = h.take_song() {
            offer(&shared, &out, song);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_library_as_native_ts_lays_it_out() {
        let mut bytes = Vec::new();
        bytes.extend(1u32.to_le_bytes());
        for w in [7u32, 2, 2, 1] {
            bytes.extend(w.to_le_bytes());
        }
        bytes.extend(30f64.to_le_bytes());
        for w in [1u32, 2, 3, 4] {
            bytes.extend(w.to_le_bytes());
        }
        bytes.extend(0.5f64.to_le_bytes());
        bytes.extend(1f64.to_le_bytes());
        bytes.extend((-1i32).to_le_bytes());
        let library = decode_library(&bytes).unwrap();
        let song = library.song(7).unwrap();
        assert_eq!((song.beats.as_slice(), song.chords.as_slice(), song.seconds), ([0.5, 1.0].as_slice(), [None].as_slice(), 30.0));
        assert!(decode_library(&bytes[..bytes.len() - 1]).is_err());
    }
}
