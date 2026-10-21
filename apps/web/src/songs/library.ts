// The player's own songs, kept in IndexedDB next to the built-in ones. Songs
// saved by earlier versions in localStorage move over on first load.

import { STYLES } from '@midzer/audio-web';
import { store } from '../storage/db';
import type { AnalysedTrack } from './audio';
import { BUILTIN_SONGS } from './builtin';
import type { MelodyNote } from './melody';
import { deleteRecording } from './recordings';
import { BEATS_PER_BAR, MAX_TRACK_BEATS, type Bar, type Song, type SongBacking, type SongSection } from './song';

const LEGACY_KEY = 'midzer.songs';
const MAX_MELODY_NOTES = 8192;

interface Saved {
  song: Song;
  /** When the song was first saved; newest songs list first. */
  savedAt: number;
}

const songs = store<Saved>('songs');
const SECTIONS: SongSection[] = ['intro', 'A', 'B', 'C', 'ending'];

/** Keeps only well-formed songs, so a damaged entry can't break the app. */
function sanitize(value: unknown): Song | null {
  if (typeof value !== 'object' || value === null) return null;
  const s = value as Partial<Song>;
  if (typeof s.id !== 'string' || typeof s.title !== 'string' || !Array.isArray(s.bars)) return null;
  const bars: Bar[] = s.bars.flatMap((b: Partial<Bar>) => {
    if (!Array.isArray(b?.beats) || b.beats.length !== BEATS_PER_BAR) return [];
    const bar: Bar = { beats: b.beats.map((x) => (typeof x === 'string' ? x : '')) };
    if (b.section && SECTIONS.includes(b.section)) bar.section = b.section;
    if (b.fill === true) bar.fill = true;
    if (typeof b.lyric === 'string') bar.lyric = b.lyric;
    return [bar];
  });
  return {
    id: s.id,
    title: s.title,
    artist: typeof s.artist === 'string' ? s.artist : undefined,
    bpm: typeof s.bpm === 'number' && s.bpm >= 40 && s.bpm <= 240 ? s.bpm : 100,
    style: Number.isInteger(s.style) && STYLES[s.style!] ? s.style! : 0,
    bars,
    melody: Array.isArray(s.melody) ? sanitizeMelody(s.melody) : undefined,
    source: s.source === 'chordpro' || s.source === 'midi' || s.source === 'audio' ? s.source : 'user',
    transcription: Array.isArray(s.transcription) ? sanitizeTracks(s.transcription) : undefined,
    backing: sanitizeBacking(s.backing),
  };
}

function sanitizeBacking(value: unknown): SongBacking | undefined {
  const { beatTimes, seconds, stemsDir } = (value ?? {}) as Partial<SongBacking>;
  if (!Array.isArray(beatTimes) || typeof seconds !== 'number' || !(seconds > 0)) return undefined;
  const times = beatTimes.slice(0, MAX_TRACK_BEATS);
  // Beat times only ever move forward.
  if (times.length < 2 || !times.every((t, i) => typeof t === 'number' && t >= 0 && (i === 0 || t > times[i - 1]!))) return undefined;
  return typeof stemsDir === 'string' ? { beatTimes: times, seconds, stemsDir } : { beatTimes: times, seconds };
}

function sanitizeTracks(tracks: unknown[]): AnalysedTrack[] | undefined {
  const ok = tracks.slice(0, 16).flatMap((t) => {
    const { name, program, drums, notes } = (t ?? {}) as Partial<AnalysedTrack>;
    if (typeof name !== 'string' || !Number.isInteger(program) || !Array.isArray(notes)) return [];
    const valid = notes.filter(
      (n) => typeof n?.at === 'number' && typeof n.len === 'number' && Number.isInteger(n.key) && Number.isInteger(n.velocity),
    );
    return [{ name, program: program!, drums: drums === true, notes: valid }];
  });
  return ok.length ? ok : undefined;
}

function sanitizeMelody(notes: unknown[]): MelodyNote[] | undefined {
  const ok = notes.slice(0, MAX_MELODY_NOTES).flatMap((n) => {
    const { at, len, key } = (n ?? {}) as Partial<MelodyNote>;
    const valid = typeof at === 'number' && at >= 0 && typeof len === 'number' && len > 0 && Number.isInteger(key) && key! >= 0 && key! < 128;
    return valid ? [{ at, len, key: key! }] : [];
  });
  return ok.length ? ok : undefined;
}

let migrated: Promise<void> | null = null;

/** Moves songs saved by earlier versions out of localStorage, once. */
function migrate(): Promise<void> {
  migrated ??= (async () => {
    let legacy: unknown;
    try {
      legacy = JSON.parse(localStorage.getItem(LEGACY_KEY) ?? 'null');
    } catch {
      return;
    }
    if (!Array.isArray(legacy)) return;
    // The old list was newest first.
    const now = Date.now();
    for (const [i, raw] of legacy.entries()) {
      const song = sanitize(raw);
      if (song && !(await songs.get(song.id))) await songs.put(song.id, { song, savedAt: now - i });
    }
    try {
      localStorage.removeItem(LEGACY_KEY);
    } catch {
      // Stays; the songs are only copied again if missing.
    }
  })();
  return migrated;
}

export async function loadUserSongs(): Promise<Song[]> {
  try {
    await migrate();
    const saved = await songs.all();
    return saved
      .sort((a, b) => b.savedAt - a.savedAt)
      .flatMap((s) => sanitize(s.song) ?? []);
  } catch {
    return [];
  }
}

/** Adds or replaces a song by id. Resolves false if it could not be stored. */
export async function saveUserSong(song: Song): Promise<boolean> {
  try {
    await migrate();
    const existing = await songs.get(song.id);
    await songs.put(song.id, { song, savedAt: existing?.savedAt ?? Date.now() });
    return true;
  } catch {
    return false;
  }
}

export async function deleteUserSong(id: string): Promise<void> {
  void deleteRecording(id);
  try {
    await songs.delete(id);
  } catch {
    // Nothing to do; it stays listed.
  }
}

export function isBuiltin(song: Song): boolean {
  return BUILTIN_SONGS.some((s) => s.id === song.id);
}
