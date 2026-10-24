// Recordings → songs. The analysis itself runs in Rust (crates/midzer-analyze):
// in the browser as wasm in a worker, in the desktop app natively, where it can
// also separate the instruments first (crates/midzer-stems). This file turns
// its result into a chart, and its transcribed parts into a MIDI file.

import { NOTE_NAMES } from '@midzer/audio-web';
import { formatChord } from './chords';
import type { MelodyNote } from './melody';
import { tempoEvent, writeSmf, type TrackEvents } from './smf';
import { BEATS_PER_BAR, MAX_BARS, MAX_TRACK_BEATS, newSongId, type Bar, type Song, type SongSection } from './song';

/** Mirrors `Analysis` in crates/midzer-analyze/src/lib.rs. */
export interface AudioAnalysis {
  title: string | null;
  artist: string | null;
  duration: number;
  bpm: number;
  key: { tonic: number; minor: boolean };
  tuningCents: number;
  barTimes: number[];
  /** Every beat of the bars, plus the end of the last one. */
  beatTimes: number[];
  /** Four per bar; null is no chord. */
  chords: ({ root: number; quality: number; bass: number } | null)[];
  sections: { bar: number; part: SongSection }[];
  melody: AnalysedNote[];
  melodyFrom: 'mix' | 'vocals' | null;
  tracks: AnalysedTrack[];
  stemsDir: string | null;
  warnings: string[];
  /** The level, 100 a second, 0–255 for −60–0 dBFS. */
  envelope: number[];
  /** Landmark hashes: hash, frame, hash, frame, … */
  fingerprint: number[];
}

export interface AnalysedNote {
  at: number;
  len: number;
  key: number;
  velocity: number;
}

export interface AnalysedTrack {
  name: string;
  program: number;
  drums: boolean;
  notes: AnalysedNote[];
}

/** What the analysis is doing; mirrors `Step` in lib.rs. */
export const STEP_LABELS = [
  'Reading the file',
  'Finding the beat',
  'Hearing the chords',
  'Finding the sections',
  'Following the melody',
  'Separating the instruments',
  'Writing down the notes',
] as const;

const AUDIO_EXTENSIONS = ['mp3', 'm4a', 'mp4', 'aac', 'wav', 'flac', 'ogg', 'oga'];
export const AUDIO_ACCEPT = AUDIO_EXTENSIONS.map((e) => `.${e}`).join(',');

/** Whether a file looks like a recording, by its name or its first bytes. */
export function isAudioFile(name: string, head: Uint8Array): boolean {
  const ext = name.toLowerCase().split('.').pop() ?? '';
  if (AUDIO_EXTENSIONS.includes(ext)) return true;
  const ascii = (from: number, to: number) => String.fromCharCode(...head.slice(from, to));
  return (
    ascii(0, 3) === 'ID3' ||
    (head[0] === 0xff && ((head[1] ?? 0) & 0xe0) === 0xe0) || // an MPEG frame
    (ascii(0, 4) === 'RIFF' && ascii(8, 12) === 'WAVE') ||
    ascii(0, 4) === 'fLaC' ||
    ascii(0, 4) === 'OggS' ||
    ascii(4, 8) === 'ftyp'
  );
}

export interface FromAudio {
  song: Song;
  notes: string[];
  /** To keep with the song (see fingerprints.ts). */
  fingerprint: Uint32Array;
}

/** How a song learned from live audio was heard. */
export interface HeardLive {
  complete: boolean;
  heardAt: number;
}

/** A song from a recording's analysis, or with `live` from a song heard live, which keeps no recording. */
export function songFromAnalysis(a: AudioAnalysis, fallbackTitle: string, live?: HeardLive): FromAudio {
  const barCount = Math.min(MAX_BARS, Math.floor(a.chords.length / BEATS_PER_BAR));
  const bars: Bar[] = [];
  let previous: string | null = null;
  for (let b = 0; b < barCount; b++) {
    const beats = ['', '', '', ''];
    for (let i = 0; i < BEATS_PER_BAR; i++) {
      const chord = a.chords[b * BEATS_PER_BAR + i];
      const name = chord ? formatChord(chord) : 'N.C.';
      if (name !== previous && !(previous === null && name === 'N.C.')) beats[i] = name;
      previous = name;
    }
    bars.push({ beats });
  }
  for (const { bar, part } of a.sections) {
    if (bars[bar]) bars[bar].section = part;
  }
  const melody: MelodyNote[] = a.melody.map(({ at, len, key }) => ({ at, len, key }));
  const song: Song = {
    id: newSongId(),
    title: a.title?.trim() || fallbackTitle,
    bpm: Math.round(Math.min(240, Math.max(40, a.bpm))),
    style: 0,
    bars,
    source: live ? 'live' : 'audio',
  };
  if (a.artist?.trim()) song.artist = a.artist.trim();
  if (melody.length > 0) song.melody = melody;
  if (a.tracks.length > 0) song.transcription = a.tracks;
  if (a.beatTimes.length >= 2) {
    const beatTimes = a.beatTimes.slice(0, Math.min(MAX_TRACK_BEATS, barCount * BEATS_PER_BAR + 1));
    if (live) {
      song.heard = { beatTimes, seconds: a.duration, complete: live.complete, envelope: Uint8Array.from(a.envelope), heardAt: live.heardAt };
    } else {
      song.backing = { beatTimes, seconds: a.duration };
      if (a.stemsDir) song.backing.stemsDir = a.stemsDir;
    }
  }

  const key = `${NOTE_NAMES[a.key.tonic]}${a.key.minor ? ' minor' : ' major'}`;
  const from = live ? (live.complete ? 'Heard live' : 'Heard live (only part of the song)') : 'Read from the recording';
  const notes = [`${from}: ${song.bpm} BPM, in ${key}. Check the chords and sections in the editor.`];
  if (a.melodyFrom === 'mix') notes.push('The melody for Learn is a rough guess from the whole mix.');
  if (a.melodyFrom === 'vocals') notes.push('The melody for Learn comes from the separated vocals.');
  if (a.stemsDir) notes.push(`The separated instruments are saved as WAV files in ${a.stemsDir}.`);
  return { song, notes: [...notes, ...a.warnings], fingerprint: Uint32Array.from(a.fingerprint) };
}

const PPQ = 480;
const DRUM_CHANNEL = 9;
const MELODIC_CHANNELS = [0, 1, 2, 3, 4, 5, 6, 7, 8, 10, 11, 12, 13, 14, 15];

/** The transcribed parts as a format-1 MIDI file at the song's tempo, one track each. */
export function transcriptionMidi(tracks: AnalysedTrack[], bpm: number): Uint8Array {
  const tempo: TrackEvents = { name: 'Tempo', events: [tempoEvent(0, bpm)] };
  let melodic = 0;
  const parts = tracks.map((track): TrackEvents => {
    const ch = track.drums ? DRUM_CHANNEL : MELODIC_CHANNELS[melodic++ % MELODIC_CHANNELS.length]!;
    const events: number[][] = track.drums ? [] : [[0, 0xc0 | ch, track.program & 0x7f]];
    for (const n of track.notes) {
      const on = Math.max(0, Math.round(n.at * PPQ));
      const off = Math.max(on + 1, Math.round((n.at + n.len) * PPQ));
      events.push([on, 0x90 | ch, n.key & 0x7f, Math.max(1, Math.min(127, n.velocity))]);
      events.push([off, 0x80 | ch, n.key & 0x7f, 0]);
    }
    return { name: track.name, events };
  });
  return writeSmf(PPQ, [tempo, ...parts]);
}
