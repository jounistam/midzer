// The player's own songs, kept in localStorage next to the built-in ones.

import { STYLES } from '@midzer/audio-web';
import { BUILTIN_SONGS } from './builtin';
import { BEATS_PER_BAR, type Bar, type Song, type SongSection } from './song';

const KEY = 'midzer.songs';
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
    source: s.source === 'chordpro' || s.source === 'midi' ? s.source : 'user',
  };
}

export function loadUserSongs(): Song[] {
  try {
    const raw: unknown = JSON.parse(localStorage.getItem(KEY) ?? '[]');
    return Array.isArray(raw) ? raw.flatMap((s) => sanitize(s) ?? []) : [];
  } catch {
    return [];
  }
}

function store(songs: Song[]): boolean {
  try {
    localStorage.setItem(KEY, JSON.stringify(songs));
    return true;
  } catch {
    return false;
  }
}

/** Adds or replaces a song by id. Returns false if it could not be stored. */
export function saveUserSong(song: Song): boolean {
  const songs = loadUserSongs();
  const i = songs.findIndex((s) => s.id === song.id);
  if (i >= 0) songs[i] = song;
  else songs.unshift(song);
  return store(songs);
}

export function deleteUserSong(id: string): void {
  store(loadUserSongs().filter((s) => s.id !== id));
}

export function isBuiltin(song: Song): boolean {
  return BUILTIN_SONGS.some((s) => s.id === song.id);
}
