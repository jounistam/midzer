import { describe, expect, it } from 'vitest';
import { chordIn, importMidi } from './midiImport';
import { readSmf, tempoEvent, writeSmf } from './smf';

/** A file with chords (one per bar, or as given) on channel 0 and drums on channel 9. */
function chordFile(chords: number[][], { ppq = 96, bpm = 90, meter = [4, 2] } = {}): Uint8Array {
  const bar = ppq * 4;
  const events: number[][] = [];
  chords.forEach((keys, i) => {
    for (const key of keys) {
      events.push([i * bar, 0x90, key, 80], [(i + 1) * bar - 1, 0x80, key, 0]);
    }
    events.push([i * bar, 0x99, 36, 100], [i * bar + 10, 0x89, 36, 0]);
  });
  return writeSmf(ppq, [
    { name: 'Conductor', events: [tempoEvent(0, bpm), [0, 0xff, 0x58, meter[0]!, meter[1]!, 24, 8]] },
    { name: 'Piano', events },
  ]);
}

describe('SMF', () => {
  it('round-trips notes, tempo, meter and names', () => {
    const smf = readSmf(chordFile([[60, 64, 67]], { bpm: 120 }));
    expect(smf.ppq).toBe(96);
    expect(smf.tempos).toEqual([{ tick: 0, usPerQuarter: 500_000 }]);
    expect(smf.meters).toEqual([{ tick: 0, numerator: 4, denominator: 4 }]);
    expect(smf.tracks.map((t) => t.name)).toEqual(['Conductor', 'Piano']);
    expect(smf.tracks[1]!.notes).toContainEqual({ tick: 0, duration: 383, channel: 0, key: 64, velocity: 80 });
  });

  it('handles running status and note-on with velocity 0', () => {
    // MThd, format 0, one track, 96 ppq; notes C and E with running status.
    const bytes = new Uint8Array([
      ...[0x4d, 0x54, 0x68, 0x64, 0, 0, 0, 6, 0, 0, 0, 1, 0, 96],
      ...[0x4d, 0x54, 0x72, 0x6b, 0, 0, 0, 17],
      ...[0, 0x90, 60, 100, 0, 64, 90, 96, 60, 0, 0, 64, 0, 0, 0xff, 0x2f, 0],
    ]);
    const notes = readSmf(bytes).tracks[0]!.notes;
    expect(notes.map((n) => [n.key, n.duration])).toEqual([
      [60, 96],
      [64, 96],
    ]);
  });

  it('rejects files that are not MIDI', () => {
    expect(() => readSmf(new TextEncoder().encode('hello world!'))).toThrow('not a MIDI file');
  });
});

describe('importMidi', () => {
  it('reads one chord per bar, skipping drums', () => {
    const { song, warnings } = importMidi(
      chordFile([
        [48, 60, 64, 67],
        [45, 60, 64, 69],
        [41, 60, 65, 69],
        [43, 59, 62, 65, 67],
        [43, 59, 62, 65, 67],
      ]),
      'Test',
    );
    expect(warnings).toEqual([]);
    expect(song.bpm).toBe(90);
    expect(song.bars.map((b) => b.beats.filter(Boolean).join(' '))).toEqual(['C', 'Am', 'F', 'G7', '']);
  });

  it('splits bars whose halves differ and warns about other meters', () => {
    const ppq = 96;
    const events = [
      [0, 0x90, 60, 80],
      [0, 0x90, 64, 80],
      [0, 0x90, 67, 80],
      [ppq * 2, 0x80, 60, 0],
      [ppq * 2, 0x80, 64, 0],
      [ppq * 2, 0x80, 67, 0],
      [ppq * 2, 0x90, 62, 80],
      [ppq * 2, 0x90, 65, 80],
      [ppq * 2, 0x90, 69, 80],
      [ppq * 4, 0x80, 62, 0],
      [ppq * 4, 0x80, 65, 0],
      [ppq * 4, 0x80, 69, 0],
    ];
    const file = writeSmf(ppq, [{ name: '', events: [[0, 0xff, 0x58, 3, 2, 24, 8], ...events] }]);
    const { song, warnings } = importMidi(file, '');
    expect(song.bars[0]!.beats).toEqual(['C', '', 'Dm', '']);
    expect(song.title).toBe('Imported song');
    expect(warnings[0]).toMatch(/3\/4/);
  });

  it('names inversions and ignores weak passing notes', () => {
    const note = (key: number, tick = 0, duration = 100) => ({ tick, duration, channel: 0, key, velocity: 80 });
    expect(chordIn([note(52), note(60), note(67)], 0, 100)).toBe('C/E');
    expect(chordIn([note(48), note(64), note(67), note(62, 0, 10)], 0, 100)).toBe('C');
    expect(chordIn([], 0, 100)).toBeNull();
  });

  it('fails clearly without notes', () => {
    expect(() => importMidi(writeSmf(96, [{ name: 'x', events: [] }]), 'x')).toThrow(/no notes/);
  });
});

describe('melody from MIDI', () => {
  it('takes the top line of the highest single-note part, in beats from the first bar', () => {
    const ppq = 96;
    const chords: number[][] = [];
    for (let bar = 1; bar <= 3; bar++) {
      for (const key of [48, 52, 55]) chords.push([bar * ppq * 4, 0x90, key, 70], [(bar + 1) * ppq * 4 - 1, 0x80, key, 0]);
    }
    // A tune starting in bar 1: twelve quarter notes, the third one doubled a third below and a little late.
    const tune = [64, 62, 60, 62, 64, 64, 64, 62, 62, 64, 67, 67];
    const lead: number[][] = [];
    tune.forEach((key, i) => {
      const tick = ppq * 4 + i * ppq + (i === 2 ? 3 : 0);
      lead.push([tick, 0x91, key, 90], [tick + ppq - 8, 0x81, key, 0]);
      if (i === 2) lead.push([tick, 0x91, 57, 60], [tick + ppq - 8, 0x81, 57, 0]);
    });
    const file = writeSmf(ppq, [
      { name: 'Chords', events: chords },
      { name: 'Lead', events: lead },
    ]);
    const { song, melodyFrom } = importMidi(file, 'Tune');
    expect(melodyFrom).toBe('Lead');
    expect(song.melody?.map((n) => n.key)).toEqual(tune);
    expect(song.melody?.slice(0, 3)).toEqual([
      { at: 0, len: 1, key: 64 },
      { at: 1, len: 1, key: 62 },
      { at: 2, len: 1, key: 60 },
    ]);
  });

  it('finds no melody in chords alone', () => {
    expect(importMidi(chordFile([[48, 60, 64, 67], [45, 60, 64, 69]]), 'x').song.melody).toBeUndefined();
  });
});
