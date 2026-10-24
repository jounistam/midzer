// The song model shared by Songs and Learn: a chord chart in 4/4 with
// sections, plus a compact text notation for built-in songs and the editor.
//
// Chart notation: bars are separated by `|` or line breaks. A bar holds one, two or four
// chords (whole bar, half bars, beats); `.` keeps the previous chord for a
// beat, `%` repeats the previous bar and `N.C.` is no chord. A tag in square
// brackets starts a section at its bar: [Intro], [A], [B], [C], [Ending] (also
// [Verse], [Chorus], [Bridge], [Outro]); [Fill] asks for a fill in that bar.

import type { EngineCommand, SongMark } from '@midzer/audio-web';
import { isNoChord, parseChord } from './chords';
import type { AnalysedTrack } from './audio';
import type { MelodyNote } from './melody';

export const BEATS_PER_BAR = 4;
/** Mirrors `MAX_SONG_BARS` in song.rs. */
export const MAX_BARS = 512;

export type SongSection = 'intro' | 'A' | 'B' | 'C' | 'ending';

export const SECTION_LABELS: Record<SongSection, string> = {
  intro: 'Intro',
  A: 'A',
  B: 'B',
  C: 'C',
  ending: 'Ending',
};

export interface Bar {
  /** One chord name per beat; '' keeps the chord before it. */
  beats: string[];
  /** A section that starts at this bar. */
  section?: SongSection;
  /** A drum fill in the second half of the bar. */
  fill?: boolean;
  /** Words sung over this bar, from ChordPro. */
  lyric?: string;
}

export type SongSource = 'builtin' | 'user' | 'chordpro' | 'midi' | 'audio' | 'live';

export interface Song {
  id: string;
  title: string;
  artist?: string;
  bpm: number;
  /** Index into STYLES. */
  style: number;
  bars: Bar[];
  /** The tune, for Learn; from the song's notation or a MIDI file's melody track. */
  melody?: MelodyNote[];
  /** Parts transcribed from a recording, for MIDI export. */
  transcription?: AnalysedTrack[];
  /** The recording the song was read from, to play along with (kept in storage/db under the song's id). */
  backing?: SongBacking;
  /** How the chart lines up with the song as it was heard live; no recording is kept. */
  heard?: SongHeard;
  source: SongSource;
}

/** Mirrors `MAX_TRACK_BEATS` in backing.rs: every beat of the longest chart, plus its end. */
export const MAX_TRACK_BEATS = MAX_BARS * BEATS_PER_BAR + 1;

/** Loads a recording's tempo map into the engine. */
export function trackCommands(backing: SongBacking): EngineCommand[] {
  const times = backing.beatTimes.slice(0, MAX_TRACK_BEATS);
  return [{ type: 'trackClear', beats: times.length }, ...times.map((seconds, index): EngineCommand => ({ type: 'trackBeat', index, seconds }))];
}

/** How the chart lines up with its recording. */
export interface SongBacking {
  /** When each chart beat is heard, in seconds into the recording, plus the end of the last bar. */
  beatTimes: number[];
  /** The recording's length in seconds. */
  seconds: number;
  /** Where the desktop app saved the separated instruments, if it did. */
  stemsDir?: string;
}

/** A song learned from live audio: its beat map, and enough to draw it without the audio. */
export interface SongHeard {
  /** When each chart beat was heard, in seconds from the start of what was captured, plus the end of the last bar. */
  beatTimes: number[];
  /** How long the capture is, in seconds. */
  seconds: number;
  /** Heard from its start to its end; otherwise only part of the song. */
  complete: boolean;
  /** The level, 100 a second, from −60 dBFS (0) to 0 dBFS (255). */
  envelope: Uint8Array;
  /** When it was heard, in ms since 1970. */
  heardAt: number;
}

const TAGS: Record<string, SongSection | 'fill'> = {
  intro: 'intro',
  a: 'A',
  verse: 'A',
  b: 'B',
  chorus: 'B',
  c: 'C',
  bridge: 'C',
  ending: 'ending',
  outro: 'ending',
  end: 'ending',
  fill: 'fill',
};

/** Maps a section name such as "Chorus 2" or "verse" to a section, if it names one. */
export function sectionOf(name: string): SongSection | 'fill' | undefined {
  const word = name.trim().toLowerCase().split(/[\s\d:]+/)[0] ?? '';
  return TAGS[word];
}

export function emptyBar(): Bar {
  return { beats: ['', '', '', ''] };
}

/** Spreads 1, 2 or 4 chords over the beats; other counts go one per beat. */
function spread(tokens: string[]): string[] {
  const beats = ['', '', '', ''];
  const step = tokens.length === 1 ? 4 : tokens.length === 2 ? 2 : 1;
  tokens.slice(0, BEATS_PER_BAR).forEach((t, i) => (beats[i * step] = t === '.' ? '' : t));
  return beats;
}

/** A chord token as stored: the original spelling, or 'N.C.'. */
function chordToken(token: string): string | null {
  if (token === '.') return '.';
  if (isNoChord(token)) return 'N.C.';
  return parseChord(token) ? token : null;
}

export interface ParseResult {
  bars: Bar[];
  /** Tokens that are not chords, e.g. typos. */
  unknown: string[];
}

/** Parses chart notation. Unknown tokens are reported and skipped. */
export function parseChart(text: string): ParseResult {
  const bars: Bar[] = [];
  const unknown: string[] = [];
  let section: SongSection | undefined;
  let fill = false;
  for (const cell of text.split(/[|\n]/)) {
    const tokens: string[] = [];
    for (const raw of cell.match(/\[[^\]]*\]|\S+/g) ?? []) {
      if (raw.startsWith('[')) {
        const tag = sectionOf(raw.slice(1, -1));
        if (tag === 'fill') fill = true;
        else if (tag) section = tag;
        else unknown.push(raw);
      } else if (raw === '%') {
        const prev = bars[bars.length - 1];
        if (prev) tokens.push(...prev.beats.map((b) => b || '.'));
      } else {
        const token = chordToken(raw);
        if (token) tokens.push(token);
        else unknown.push(raw);
      }
    }
    if (tokens.length === 0) continue; // empty cells, e.g. from "||" or a trailing bar line
    const bar: Bar = { beats: spread(tokens) };
    if (section) bar.section = section;
    if (fill) bar.fill = true;
    bars.push(bar);
    section = undefined;
    fill = false;
  }
  return { bars, unknown };
}

/** One bar in chart notation, without its tags: "C", "Am F" or "C . G7 .". */
export function formatBar(bar: Bar): string {
  const b = bar.beats;
  if (b.every((x) => x === '')) return '.';
  if (b[1] === '' && b[2] === '' && b[3] === '') return b[0] || '.';
  if (b[1] === '' && b[3] === '') return `${b[0] || '.'} ${b[2]}`;
  return b.map((x) => x || '.').join(' ');
}

/** The whole chart in notation, four bars a line. */
export function formatChart(bars: Bar[]): string {
  const lines: string[] = [];
  let line: string[] = [];
  bars.forEach((bar, i) => {
    if (bar.section && line.length > 0) {
      lines.push(line.join(' | '));
      line = [];
    }
    const tags = `${bar.section ? `[${SECTION_LABELS[bar.section]}] ` : ''}${bar.fill ? '[Fill] ' : ''}`;
    line.push(tags + formatBar(bar));
    if (line.length === 4 || i === bars.length - 1) {
      lines.push(line.join(' | '));
      line = [];
    }
  });
  return lines.join('\n');
}

/** The chord name heard on each beat, holds resolved; null before the first chord or at N.C. */
export function resolvedBeats(bars: Bar[]): (string | null)[] {
  const out: (string | null)[] = [];
  let current: string | null = null;
  for (const bar of bars) {
    for (const beat of bar.beats) {
      if (beat) current = beat === 'N.C.' ? null : beat;
      out.push(current);
    }
  }
  return out;
}

/** The section in force at each bar. */
export function sectionsByBar(bars: Bar[]): (SongSection | undefined)[] {
  let current: SongSection | undefined;
  return bars.map((bar) => (current = bar.section ?? current));
}

/**
 * Engine marks per bar. Intro bars each play the intro; A, B and C switch the
 * variation where they start; an ending section plays the ending in its last
 * bar (the band stops after it). Fills go where nothing else is asked for.
 */
export function marksByBar(bars: Bar[]): SongMark[] {
  const sections = sectionsByBar(bars);
  return bars.map((bar, i) => {
    const section = sections[i];
    if (section === 'intro') return 'intro';
    if (section === 'ending' && sections[i + 1] !== 'ending') return 'ending';
    if (bar.section && bar.section !== 'ending') return bar.section as SongMark;
    return bar.fill ? 'fill' : null;
  });
}

/** Commands that load the chart into the engine. Only non-empty entries are sent. */
export function chartCommands(song: Pick<Song, 'bars'>): EngineCommand[] {
  const bars = song.bars.slice(0, MAX_BARS);
  const cmds: EngineCommand[] = [{ type: 'songClear', bars: bars.length }];
  bars.forEach((bar, b) =>
    bar.beats.forEach((token, i) => {
      if (!token) return;
      const spec = token === 'N.C.' ? 'nc' : parseChord(token);
      if (spec) cmds.push({ type: 'songChord', beat: b * BEATS_PER_BAR + i, slot: spec });
    }),
  );
  marksByBar(bars).forEach((mark, bar) => mark && cmds.push({ type: 'songMark', bar, mark }));
  return cmds;
}

export function newSongId(): string {
  return `song-${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 7)}`;
}

/** Builds a song from chart notation; for built-ins and tests. */
export function songFromChart(fields: Omit<Song, 'bars'>, chart: string): Song {
  return { ...fields, bars: parseChart(chart).bars };
}
