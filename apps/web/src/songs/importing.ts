// Turns a dropped file or pasted text into a song, whatever its format.

import { parseChordPro } from './chordpro';
import { importMidi } from './midiImport';
import { newSongId, parseChart, type Song } from './song';

export interface Imported {
  song: Song;
  /** Things to tell the user: unreadable chords, a different meter, … */
  notes: string[];
}

export const titleOf = (fileName: string) => fileName.replace(/\.[^.]+$/, '').replace(/[_-]+/g, ' ').trim();

/** Reads ChordPro, or chart notation ("C | Am | F | G") when there are no ChordPro markings. */
export function importText(text: string, title = 'Untitled song'): Imported {
  const chordPro = /^\s*\{[\w-]+.*\}\s*$/m.test(text) || /\[[^\]|]+\][^\s|\]]/.test(text);
  if (!chordPro) {
    const { bars, unknown } = parseChart(text);
    if (bars.length > 0) {
      const song: Song = { id: newSongId(), title, bpm: 100, style: 0, bars, source: 'user' };
      return { song, notes: unknownNote(unknown) };
    }
  }
  const { song, unknown } = parseChordPro(text, title);
  if (song.bars.length === 0) throw new Error('No chords found. Chords go in square brackets, like [C] or [Am7].');
  return { song, notes: unknownNote(unknown) };
}

export async function importFile(file: File): Promise<Imported> {
  const bytes = new Uint8Array(await file.arrayBuffer());
  const title = titleOf(file.name);
  const isMidi = bytes[0] === 0x4d && bytes[1] === 0x54 && bytes[2] === 0x68 && bytes[3] === 0x64; // "MThd"
  if (isMidi) {
    const { song, warnings, melodyFrom } = importMidi(bytes, title);
    const melody = melodyFrom ? [`The melody for Learn comes from ${melodyFrom}.`] : [];
    return { song, notes: ['Chords were read from the notes; check them in the editor.', ...melody, ...warnings] };
  }
  return importText(new TextDecoder().decode(bytes), title);
}

function unknownNote(unknown: string[]): string[] {
  const unique = [...new Set(unknown)];
  if (unique.length === 0) return [];
  const list = unique.slice(0, 5).join(', ') + (unique.length > 5 ? ', …' : '');
  return [`Skipped what didn't read as chords: ${list}`];
}
