//! Recognising a stored song from what is heard: the landmarks of the last
//! seconds (see [`crate::fingerprint`]) are looked up in the landmarks of the
//! songs kept, and every one found votes for a song and a time offset. Music
//! heard before gives many votes on one offset, which says where in the song
//! the stream is; other music gives a few, spread out.
//!
//! The library also keeps each song's beat map and its chord per beat, so
//! the listener can follow a song once it is recognised.

use std::collections::HashMap;

use midzer_core::{Chord, Quality};

use crate::fingerprint::Landmark;

/// Votes a match needs, from about 8 s heard (real songs give hundreds),
/// and how many times more than any other song it must have.
pub const MATCH_VOTES: u32 = 25;
const MATCH_LEAD: u32 = 4;
/// Places in the song with this share of the best's votes are as likely
/// (a part that repeats); the earliest is taken, to follow it the longest.
const SAME_PLACE: f32 = 0.8;
/// Stored frames past this (25 min) aren't indexed.
const MAX_FRAME: u32 = u16::MAX as u32;

/// A song kept, to recognise and follow.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct StoredSong {
    /// The app's number for it.
    pub id: u32,
    /// When each chart beat falls, in seconds from the start of the audio
    /// its landmarks were made from, plus the end of the last bar. Chart beat
    /// 0 is a 1.
    pub beats: Vec<f64>,
    /// The chord of each beat.
    pub chords: Vec<Option<Chord>>,
    /// How long the audio was.
    pub seconds: f64,
}

/// A song recognised: which, where, and how sure.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Match {
    pub song: u32,
    /// The song's landmark frame less the heard one's (fractional: an average
    /// over the votes either side of the best offset).
    pub offset: f64,
    pub votes: u32,
}

/// A chord from its code, root · 16 + the engine's quality index (−1 or anything unknown is none).
pub fn chord_of_code(code: i32) -> Option<Chord> {
    let quality = *Quality::ALL.get(usize::try_from(code).ok()? % 16)?;
    let root = (code / 16) as u8 % 12;
    Some(Chord { root, quality, bass: root })
}

#[derive(Default)]
pub struct Library {
    songs: Vec<StoredSong>,
    landmarks: Vec<Vec<Landmark>>,
    /// Every landmark, sorted: hash << 32 | the song's slot << 16 | frame.
    index: Vec<u64>,
}

/// The votes of one look-up, by song slot and offset.
pub struct Votes {
    votes: HashMap<(u16, i32), u32>,
}

impl Library {
    pub fn is_empty(&self) -> bool {
        self.songs.is_empty()
    }

    pub fn song(&self, id: u32) -> Option<&StoredSong> {
        self.songs.iter().find(|s| s.id == id)
    }

    /// Adds a song, or replaces the one with its id.
    pub fn add(&mut self, song: StoredSong, landmarks: Vec<Landmark>) {
        self.remove_quietly(song.id);
        if self.songs.len() < u16::MAX as usize {
            self.songs.push(song);
            self.landmarks.push(landmarks);
        }
        self.reindex();
    }

    pub fn remove(&mut self, id: u32) {
        self.remove_quietly(id);
        self.reindex();
    }

    pub fn clear(&mut self) {
        self.songs.clear();
        self.landmarks.clear();
        self.index.clear();
    }

    fn remove_quietly(&mut self, id: u32) {
        if let Some(i) = self.songs.iter().position(|s| s.id == id) {
            self.songs.remove(i);
            self.landmarks.remove(i);
        }
    }

    fn reindex(&mut self) {
        self.index.clear();
        for (slot, marks) in self.landmarks.iter().enumerate() {
            self.index.extend(marks.iter().filter(|m| m.1 <= MAX_FRAME).map(|&(h, t)| (h as u64) << 32 | (slot as u64) << 16 | t as u64));
        }
        self.index.sort_unstable();
    }

    /// Every heard landmark votes for the songs and offsets it is found at.
    pub fn votes(&self, heard: &[Landmark]) -> Votes {
        let mut votes: HashMap<(u16, i32), u32> = HashMap::new();
        for &(h, t) in heard {
            let key = (h as u64) << 32;
            let from = self.index.partition_point(|&e| e < key);
            for &e in self.index[from..].iter().take_while(|&&e| e >> 32 == h as u64) {
                let (slot, frame) = ((e >> 16) as u16, (e & 0xffff) as i32);
                *votes.entry((slot, frame - t as i32)).or_default() += 1;
            }
        }
        Votes { votes }
    }

    /// The song heard, if one stands out clearly (songs in `ignore` don't count).
    pub fn recognise(&self, votes: &Votes, ignore: &[u32]) -> Option<Match> {
        let mut best: Option<(u16, i32, u32)> = None;
        // Each song's best, for the lead over the others.
        let mut per_song: HashMap<u16, u32> = HashMap::new();
        for &(slot, offset) in votes.votes.keys() {
            if ignore.contains(&self.songs[slot as usize].id) {
                continue;
            }
            let n = votes.pooled(slot, offset);
            let top = per_song.entry(slot).or_default();
            *top = (*top).max(n);
            if best.is_none_or(|b| (n, std::cmp::Reverse(offset)) > (b.2, std::cmp::Reverse(b.1))) {
                best = Some((slot, offset, n));
            }
        }
        let (slot, mut offset, n) = best?;
        let other = per_song.iter().filter(|e| *e.0 != slot).map(|e| *e.1).max().unwrap_or(0);
        if n < MATCH_VOTES || n < MATCH_LEAD * other {
            return None;
        }
        for &(s, o) in votes.votes.keys() {
            if s == slot && o < offset && votes.pooled(s, o) as f32 >= SAME_PLACE * n as f32 {
                offset = o;
            }
        }
        let n = votes.pooled(slot, offset);
        Some(Match { song: self.songs[slot as usize].id, offset: votes.centre(slot, offset), votes: n })
    }

    /// The votes song `id` has within `frames` of `offset` (pooled as in a
    /// match), and the offset they centre on.
    pub fn near(&self, votes: &Votes, id: u32, offset: f64, frames: i32) -> (u32, f64) {
        let Some(slot) = self.songs.iter().position(|s| s.id == id) else { return (0, offset) };
        let slot = slot as u16;
        let o = offset.round() as i32;
        let best = (o - frames..=o + frames).max_by_key(|&k| (votes.pooled(slot, k), std::cmp::Reverse((k - o).abs())));
        best.map_or((0, offset), |k| (votes.pooled(slot, k), votes.centre(slot, k)))
    }

    /// The last heard landmark frame that matches song `id` within `frames` of `offset`.
    pub fn last_heard(&self, heard: &[Landmark], id: u32, offset: f64, frames: i32) -> Option<u32> {
        let slot = self.songs.iter().position(|s| s.id == id)? as u64;
        let o = offset.round() as i32;
        heard.iter().rev().find_map(|&(h, t)| {
            let from = self.index.partition_point(|&e| e < ((h as u64) << 32 | slot << 16));
            let found = self.index[from..].iter().take_while(|&&e| e >> 16 == ((h as u64) << 16 | slot)).any(|&e| ((e & 0xffff) as i32 - t as i32 - o).abs() <= frames + 1);
            found.then_some(t)
        })
    }
}

impl Votes {
    fn at(&self, slot: u16, offset: i32) -> u32 {
        self.votes.get(&(slot, offset)).copied().unwrap_or(0)
    }

    /// The votes on an offset and the two beside it: a landmark's frame can
    /// come out one off.
    fn pooled(&self, slot: u16, offset: i32) -> u32 {
        (offset - 1..=offset + 1).map(|o| self.at(slot, o)).sum()
    }

    /// The offset the votes around `offset` centre on.
    fn centre(&self, slot: u16, offset: i32) -> f64 {
        let n = self.pooled(slot, offset);
        if n == 0 {
            return offset as f64;
        }
        (offset - 1..=offset + 1).map(|o| o as f64 * self.at(slot, o) as f64).sum::<f64>() / n as f64
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fingerprint::tests::tune;
    use crate::fingerprint::{FPS, RATE, fingerprint};

    fn excerpt(song: &[f32], from_s: f64, seconds: f64, seed: u32) -> Vec<f32> {
        let from = (from_s * RATE) as usize;
        let mut seed = seed;
        song[from..from + (seconds * RATE) as usize]
            .iter()
            .map(|&x| {
                seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                0.7 * x + 0.01 * ((seed >> 16) as f32 / 32768.0 - 1.0)
            })
            .collect()
    }

    #[test]
    fn a_stored_song_is_recognised_where_it_is_and_others_are_not() {
        let (a, b) = (tune(30.0, 1), tune(30.0, 3));
        let mut library = Library::default();
        library.add(StoredSong { id: 7, seconds: 30.0, ..Default::default() }, fingerprint(&a));
        library.add(StoredSong { id: 9, seconds: 30.0, ..Default::default() }, fingerprint(&b));

        let heard = fingerprint(&excerpt(&b, 12.3, 8.0, 5));
        let m = library.recognise(&library.votes(&heard), &[]).expect("a match");
        assert_eq!(m.song, 9);
        assert!((m.offset - 12.3 * FPS).abs() <= 1.0, "offset {}", m.offset);
        assert!(m.votes >= MATCH_VOTES * 2, "{} votes", m.votes);
        let (n, at) = library.near(&library.votes(&heard), 9, 12.3 * FPS + 2.0, 3);
        assert!(n == m.votes && (at - m.offset).abs() < 1e-9);
        assert!(library.recognise(&library.votes(&heard), &[9]).is_none(), "ignored");

        assert!(library.recognise(&library.votes(&fingerprint(&tune(8.0, 2))), &[]).is_none(), "other music");
        assert!(library.recognise(&library.votes(&fingerprint(&vec![0.0; 8 * RATE as usize])), &[]).is_none(), "silence");

        // A part heard twice in a song: the earliest place.
        let twice: Vec<f32> = b[..(16.0 * RATE) as usize].iter().chain(&b[..(16.0 * RATE) as usize]).copied().collect();
        let mut repeats = Library::default();
        repeats.add(StoredSong { id: 3, seconds: 32.0, ..Default::default() }, fingerprint(&twice));
        let m = repeats.recognise(&repeats.votes(&fingerprint(&excerpt(&twice, 20.0, 8.0, 5))), &[]).expect("a match");
        assert!((m.offset - 4.0 * FPS).abs() <= 1.0, "offset {}", m.offset);

        library.remove(9);
        assert!(library.recognise(&library.votes(&heard), &[]).is_none(), "removed");
        assert!(library.song(9).is_none() && library.song(7).is_some());
    }
}
