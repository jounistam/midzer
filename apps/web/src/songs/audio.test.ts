import { describe, expect, it } from 'vitest';
import { isAudioFile, songFromAnalysis, transcriptionMidi, type AudioAnalysis } from './audio';
import { Q } from './chords';
import { readSmf } from './smf';

const C = { root: 0, quality: Q.major, bass: 0 };
const Am = { root: 9, quality: Q.minor, bass: 9 };
const G7B = { root: 7, quality: Q.dom7, bass: 11 };

function analysis(over: Partial<AudioAnalysis> = {}): AudioAnalysis {
  return {
    title: null,
    artist: null,
    duration: 10,
    bpm: 96.4,
    key: { tonic: 0, minor: false },
    tuningCents: 0,
    barTimes: [0.5, 3, 5.5],
    beatTimes: Array.from({ length: 13 }, (_, i) => 0.5 + i * 0.625),
    chords: [null, C, C, C, Am, Am, G7B, G7B, C, C, C, C],
    sections: [
      { bar: 0, part: 'intro' },
      { bar: 1, part: 'A' },
    ],
    melody: [{ at: 4, len: 1, key: 64, velocity: 90 }],
    melodyFrom: 'mix',
    tracks: [],
    stemsDir: null,
    warnings: [],
    ...over,
  };
}

describe('songs from recordings', () => {
  it('writes a chord only where it changes, and keeps the sections', () => {
    const { song, notes } = songFromAnalysis(analysis(), 'My take');
    expect(song.title).toBe('My take');
    expect(song.bpm).toBe(96);
    expect(song.source).toBe('audio');
    expect(song.bars.map((b) => b.beats)).toEqual([
      ['', 'C', '', ''],
      ['Am', '', 'G7/B', ''],
      ['C', '', '', ''],
    ]);
    expect(song.bars.map((b) => b.section)).toEqual(['intro', 'A', undefined]);
    expect(song.melody).toEqual([{ at: 4, len: 1, key: 64 }]);
    expect(notes[0]).toContain('C major');
    expect(notes.join(' ')).toContain('rough guess');
  });

  it('marks silence after music as no chord, and takes the title from the tags', () => {
    const { song } = songFromAnalysis(analysis({ chords: [C, C, null, null], title: ' Tagged ', artist: 'Band' }), 'file');
    expect(song.bars[0]!.beats).toEqual(['C', '', 'N.C.', '']);
    expect(song.title).toBe('Tagged');
    expect(song.artist).toBe('Band');
  });

  it('keeps the tempo map and the stems to play along with the recording', () => {
    const { song } = songFromAnalysis(analysis({ stemsDir: '/data/stems/1' }), 'file');
    expect(song.backing?.beatTimes).toHaveLength(13);
    expect(song.backing).toMatchObject({ seconds: 10, stemsDir: '/data/stems/1' });
    expect(song.backing!.beatTimes[4]).toBeCloseTo(3);
    // Too short a map to play along with.
    expect(songFromAnalysis(analysis({ beatTimes: [0.5] }), 'file').song.backing).toBeUndefined();
  });

  it('exports the transcribed parts as MIDI on the beat grid', () => {
    const tracks = [
      { name: 'Drums', program: 0, drums: true, notes: [{ at: 0, len: 0.25, key: 36, velocity: 100 }] },
      { name: 'Bass', program: 33, drums: false, notes: [{ at: 1.5, len: 0.5, key: 40, velocity: 80 }] },
    ];
    const smf = readSmf(transcriptionMidi(tracks, 120));
    expect(smf.tempos[0]!.usPerQuarter).toBe(500_000);
    const notes = smf.tracks.flatMap((t) => t.notes.map((n) => ({ name: t.name, channel: n.channel, tick: n.tick, key: n.key })));
    expect(notes).toEqual([
      { name: 'Drums', channel: 9, tick: 0, key: 36 },
      { name: 'Bass', channel: 0, tick: 720, key: 40 },
    ]);
    expect(smf.tracks.find((t) => t.name === 'Bass')!.programs[0]!.program).toBe(33);
  });

  it('recognises recordings by name or by their first bytes', () => {
    const bytes = (s: string) => new Uint8Array([...s].map((c) => c.charCodeAt(0)));
    expect(isAudioFile('song.MP3', new Uint8Array(16))).toBe(true);
    expect(isAudioFile('x', bytes('ID3\u0004'))).toBe(true);
    expect(isAudioFile('x', bytes('RIFF\0\0\0\0WAVE'))).toBe(true);
    expect(isAudioFile('x', bytes('\0\0\0 ftypM4A '))).toBe(true);
    expect(isAudioFile('song.mid', bytes('MThd'))).toBe(false);
    expect(isAudioFile('chords.txt', bytes('[Am]words'))).toBe(false);
  });
});
