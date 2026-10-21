//! Audio files to samples: mp3, AAC/M4A, FLAC, WAV and Ogg Vorbis through symphonia.

use std::io::Cursor;

use symphonia::core::audio::SampleBuffer;
use symphonia::core::codecs::{CODEC_TYPE_NULL, DecoderOptions};
use symphonia::core::errors::Error;
use symphonia::core::formats::FormatOptions;
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::{MetadataOptions, MetadataRevision, StandardTagKey};
use symphonia::core::probe::Hint;

/// Longer recordings are cut here; a chart holds 512 bars anyway.
pub const MAX_SECONDS: u32 = 15 * 60;

/// A decoded recording in stereo (mono files have two equal channels).
pub struct Decoded {
    pub sample_rate: u32,
    pub left: Vec<f32>,
    pub right: Vec<f32>,
    pub title: Option<String>,
    pub artist: Option<String>,
    /// Whether the file was longer than [`MAX_SECONDS`].
    pub truncated: bool,
}

impl Decoded {
    pub fn mono(&self) -> Vec<f32> {
        self.left.iter().zip(&self.right).map(|(l, r)| 0.5 * (l + r)).collect()
    }

    pub fn seconds(&self) -> f32 {
        self.left.len() as f32 / self.sample_rate as f32
    }
}

pub fn decode(bytes: Vec<u8>) -> Result<Decoded, String> {
    let stream = MediaSourceStream::new(Box::new(Cursor::new(bytes)), Default::default());
    let probed = symphonia::default::get_probe()
        .format(&Hint::new(), stream, &FormatOptions::default(), &MetadataOptions::default())
        .map_err(|_| "This file isn't audio that can be read (mp3, m4a, FLAC, WAV or Ogg).".to_string())?;
    let mut format = probed.format;
    let mut probed_meta = probed.metadata;
    let (mut title, mut artist) = (None, None);
    let mut read_tags = |rev: &MetadataRevision| {
        for tag in rev.tags() {
            let value = tag.value.to_string().trim().to_string();
            match tag.std_key {
                Some(StandardTagKey::TrackTitle) if !value.is_empty() => title = Some(value),
                Some(StandardTagKey::Artist) if !value.is_empty() => artist = Some(value),
                _ => {}
            }
        }
    };
    if let Some(rev) = probed_meta.get().as_ref().and_then(|m| m.current()) {
        read_tags(rev);
    }
    if let Some(rev) = format.metadata().current() {
        read_tags(rev);
    }

    let track = format
        .tracks()
        .iter()
        .find(|t| t.codec_params.codec != CODEC_TYPE_NULL)
        .ok_or("The file has no audio track.")?;
    let track_id = track.id;
    let sample_rate = track.codec_params.sample_rate.ok_or("The file doesn't say its sample rate.")?;
    let mut decoder = symphonia::default::get_codecs()
        .make(&track.codec_params, &DecoderOptions::default())
        .map_err(|_| "The file's audio format isn't supported.".to_string())?;

    let max_frames = (MAX_SECONDS * sample_rate) as usize;
    let (mut left, mut right) = (Vec::new(), Vec::new());
    let mut buffer: Option<SampleBuffer<f32>> = None;
    let mut truncated = false;
    loop {
        let packet = match format.next_packet() {
            Ok(p) => p,
            Err(Error::IoError(_)) | Err(Error::ResetRequired) => break,
            Err(e) => return Err(format!("The file could not be read: {e}")),
        };
        if packet.track_id() != track_id {
            continue;
        }
        let decoded = match decoder.decode(&packet) {
            Ok(d) => d,
            // A damaged frame; skip it like players do.
            Err(Error::DecodeError(_)) | Err(Error::IoError(_)) => continue,
            Err(e) => return Err(format!("The file could not be decoded: {e}")),
        };
        let spec = *decoded.spec();
        let channels = spec.channels.count().max(1);
        if buffer.as_ref().is_none_or(|b| b.capacity() < decoded.capacity()) {
            buffer = Some(SampleBuffer::new(decoded.capacity() as u64, spec));
        }
        let buf = buffer.as_mut().expect("buffer was just made");
        buf.copy_interleaved_ref(decoded);
        for frame in buf.samples().chunks_exact(channels) {
            left.push(frame[0]);
            right.push(frame[if channels > 1 { 1 } else { 0 }]);
        }
        if left.len() >= max_frames {
            left.truncate(max_frames);
            right.truncate(max_frames);
            truncated = true;
            break;
        }
    }
    if left.len() < sample_rate as usize {
        return Err("The recording is too short to read a song from.".into());
    }
    Ok(Decoded { sample_rate, left, right, title, artist, truncated })
}
