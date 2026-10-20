// ChordPro import. ChordPro places chords over lyrics but says nothing about
// rhythm, so every chord gets a bar of its own unless the file has a grid
// section ({start_of_grid}) or a line written as bars ("| C | G |"). The
// result is a starting point for the chart editor.

import { parseChord } from './chords';
import { emptyBar, newSongId, parseChart, sectionOf, type Bar, type Song, type SongSection } from './song';

const DIRECTIVE = /^\{\s*([\w-]+)\s*(?::\s*(.*?))?\s*\}$/;
const ENVIRONMENTS: Record<string, SongSection> = { verse: 'A', chorus: 'B', bridge: 'C' };

export interface ChordProResult {
  song: Song;
  /** Chords that could not be read. */
  unknown: string[];
}

export function parseChordPro(text: string, fallbackTitle = 'Untitled song'): ChordProResult {
  const song: Song = { id: newSongId(), title: fallbackTitle, bpm: 100, style: 0, bars: [], source: 'chordpro' };
  const unknown: string[] = [];
  let section: SongSection | undefined;
  let inGrid = false;
  let current: Bar | null = null;

  /** Opens a bar for `chord`, starting the pending section there. */
  const startBar = (chord: string): Bar => {
    const bar: Bar = { ...emptyBar(), beats: [chord, '', '', ''] };
    if (section) bar.section = section;
    section = undefined;
    song.bars.push(bar);
    return bar;
  };
  const addBars = (bars: Bar[]) => {
    if (section && bars[0] && !bars[0].section) bars[0].section = section;
    section = undefined;
    song.bars.push(...bars);
    current = null;
  };

  for (const raw of text.split(/\r?\n/)) {
    const line = raw.trim();
    if (!line || line.startsWith('#')) continue;

    const directive = DIRECTIVE.exec(line);
    if (directive) {
      const name = directive[1]!.toLowerCase();
      const value = directive[2] ?? '';
      if (name === 'title' || name === 't') song.title = value || song.title;
      else if (name === 'artist' || name === 'subtitle' || name === 'st') song.artist ??= value || undefined;
      else if (name === 'tempo') song.bpm = clampBpm(Number.parseFloat(value)) ?? song.bpm;
      else if (name === 'start_of_grid' || name === 'sog') inGrid = true;
      else if (name === 'end_of_grid' || name === 'eog') inGrid = false;
      else if (/^(start_of_|so)/.test(name)) {
        const env = name.replace(/^start_of_|^so/, '');
        const tag = ENVIRONMENTS[env] ?? ENVIRONMENTS[{ v: 'verse', c: 'chorus', b: 'bridge' }[env] ?? ''];
        const labelled = value ? sectionOf(value) : undefined;
        section = (labelled !== 'fill' ? labelled : undefined) ?? tag ?? section;
      } else if (name === 'comment' || name === 'c' || name === 'ci' || name === 'comment_italic') {
        // "{c: Intro}" or "{c: Chorus}" labels the next section.
        const tag = sectionOf(value);
        if (tag && tag !== 'fill') section = tag;
      }
      continue;
    }

    // A section label on its own line, e.g. "Chorus:" or "[Verse 2]" in plain chord sheets.
    const label = /^\[?([A-Za-z]+(?:\s*\d+)?)\]?:?$/.exec(line);
    if (label && !parseChord(label[1]!) && sectionOf(label[1]!) && sectionOf(label[1]!) !== 'fill') {
      section = sectionOf(label[1]!) as SongSection;
      continue;
    }

    if (inGrid || /^\|.*\|$/.test(line)) {
      const grid = parseChart(line.replace(/\[([^\]]+)\]/g, '$1'));
      unknown.push(...grid.unknown);
      addBars(grid.bars);
      continue;
    }

    // A lyric line with inline [chords]: each chord opens a bar and takes
    // the words up to the next chord.
    const parts = line.split(/\[([^\]]*)\]/);
    const words = (s: string) => s.replace(/\s+/g, ' ');
    if (parts.length === 1) {
      // Lyrics without chords continue the bar before them.
      const bar = current as Bar | null;
      if (bar) bar.lyric = `${bar.lyric ?? ''} ${words(line)}`.trim();
      continue;
    }
    const lead = words(parts[0]!).trim();
    if (lead && current) current.lyric = `${current.lyric ?? ''} ${lead}`.trim();
    for (let i = 1; i < parts.length; i += 2) {
      const name = parts[i]!.trim();
      const lyric = words(parts[i + 1] ?? '').trim();
      const chord = name === '' ? null : /^n\.?c\.?$/i.test(name) ? 'N.C.' : parseChord(name) ? name : null;
      if (!chord) {
        if (name) unknown.push(name);
        if (current && lyric) current.lyric = `${current.lyric ?? ''} ${lyric}`.trim();
        continue;
      }
      current = startBar(chord);
      if (lyric) current.lyric = lyric;
    }
  }
  return { song, unknown };
}

function clampBpm(bpm: number): number | undefined {
  return Number.isFinite(bpm) ? Math.round(Math.min(240, Math.max(40, bpm))) : undefined;
}
