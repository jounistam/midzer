import { describe, expect, it } from 'vitest';
import { eventToMidi, midiToCommand } from './midi';
import { chordName, chordTones, decodeEvent, DRUM_PIECES, encodeCommand, pieceOfNote } from './protocol';

describe('encodeCommand', () => {
  it('encodes sections and modes like command.rs', () => {
    expect(encodeCommand({ type: 'section', request: 'C' })).toEqual([11, 3, 0, 0]);
    expect(encodeCommand({ type: 'section', request: 'ending' })).toEqual([11, 5, 0, 0]);
    expect(encodeCommand({ type: 'setIdleMode', mode: 'end' })).toEqual([14, 2, 0, 0]);
    expect(encodeCommand({ type: 'setStyle', index: 4 })).toEqual([10, 4, 0, 0]);
  });

  it('encodes song charts like command.rs', () => {
    expect(encodeCommand({ type: 'songClear', bars: 32 })).toEqual([17, 32, 0, 0]);
    expect(encodeCommand({ type: 'songChord', beat: 7, slot: 'hold' })).toEqual([18, 7, 0, 0]);
    expect(encodeCommand({ type: 'songChord', beat: 8, slot: 'nc' })).toEqual([18, 8, 1, 0]);
    expect(encodeCommand({ type: 'songChord', beat: 9, slot: { root: 9, quality: 11, bass: 0 } })).toEqual([
      18,
      9,
      2,
      9 | (11 << 8),
    ]);
    expect(encodeCommand({ type: 'songMark', bar: 4, mark: 'B' })).toEqual([19, 4, 3, 0]);
    expect(encodeCommand({ type: 'songMark', bar: 4, mark: null })).toEqual([19, 4, 0, 0]);
    expect(encodeCommand({ type: 'songLoop', start: 2, end: 6 })).toEqual([20, 2, 6, 0]);
    expect(encodeCommand({ type: 'setSong', on: true })).toEqual([21, 1, 0, 0]);
    expect(encodeCommand({ type: 'songSeek', bar: 12 })).toEqual([22, 12, 0, 0]);
    expect(encodeCommand({ type: 'songWait', tick: 2640 })).toEqual([23, 1, 2640, 0]);
    expect(encodeCommand({ type: 'songWait', tick: null })).toEqual([23, 0, 0, 0]);
  });

  it('encodes listening and tempo follow like command.rs', () => {
    const [op, mode, gate] = encodeCommand({ type: 'setListen', mode: 'chords', gateDb: -45 });
    expect([op, mode]).toEqual([24, 2]);
    expect(new Float32Array(new Uint32Array([gate]).buffer)[0]).toBe(-45);
    expect(encodeCommand({ type: 'setTempoFollow', on: true })).toEqual([25, 1, 0, 0]);
  });

  it('encodes drum commands like command.rs', () => {
    expect(encodeCommand({ type: 'drumNote', note: 38, velocity: 127 })).toEqual([26, 38, 127, 0]);
    expect(encodeCommand({ type: 'drumHit', piece: 'china', velocity: 1 })).toEqual([27, 16, 1, 0]);
    expect(encodeCommand({ type: 'drumHit', piece: 'kick', velocity: 90 })).toEqual([27, 0, 90, 0]);
    expect(encodeCommand({ type: 'kitNote', note: 26, piece: 'hiHatOpen' })).toEqual([28, 26, 6, 0]);
    expect(encodeCommand({ type: 'kitNote', note: 127, piece: null })).toEqual([28, 127, 0, 0]);
    expect(encodeCommand({ type: 'setInstrument', instrument: 'drums' })).toEqual([29, 1, 0, 0]);
    expect(encodeCommand({ type: 'hiHatPedal', value: 64 })).toEqual([30, 64, 0, 0]);
    expect(encodeCommand({ type: 'drumChoke', note: 49 })).toEqual([31, 49, 0, 0]);
    expect(encodeCommand({ type: 'setKitSound', on: true })).toEqual([32, 1, 0, 0]);
  });

  it('encodes drummer mode like command.rs', () => {
    expect(encodeCommand({ type: 'setCountIn', pieces: ['hiHatClosed', 'sideStick'] })).toEqual([33, (1 << 4) | (1 << 3), 0, 0]);
    expect(encodeCommand({ type: 'setCountIn', pieces: [] })).toEqual([33, 0, 0, 0]);
    expect(encodeCommand({ type: 'setBandDrums', mode: 'percussion' })).toEqual([34, 2, 0, 0]);
    expect(encodeCommand({ type: 'kitTrigger', trigger: { kind: 'note', number: 60 }, action: 'nextSection' })).toEqual([35, 0, 60, 1]);
    expect(encodeCommand({ type: 'kitTrigger', trigger: { kind: 'control', number: 64 }, action: 'startStop' })).toEqual([35, 1, 64, 4]);
    expect(encodeCommand({ type: 'kitTrigger', trigger: { kind: 'control', number: 64 }, action: null })).toEqual([35, 1, 64, 0]);
    expect(encodeCommand({ type: 'controlChange', cc: 64, value: 127 })).toEqual([36, 64, 127, 0]);
  });

  it('encodes backing tracks like command.rs', () => {
    expect(encodeCommand({ type: 'setTrack', on: true })).toEqual([37, 1, 0, 0]);
    expect(encodeCommand({ type: 'trackClear', beats: 2049 })).toEqual([38, 2049, 0, 0]);
    const [op, index, seconds] = encodeCommand({ type: 'trackBeat', index: 2048, seconds: 312.625 });
    expect([op, index]).toEqual([39, 2048]);
    expect(new Float32Array(new Uint32Array([seconds]).buffer)[0]).toBe(312.625);
    expect(encodeCommand({ type: 'setTrackDrums', on: false })).toEqual([40, 0, 0, 0]);
  });

  it('knows each piece by its General MIDI key', () => {
    expect(DRUM_PIECES).toHaveLength(17);
    expect(new Set(DRUM_PIECES.map((p) => p.note)).size).toBe(17);
    expect(pieceOfNote(38)).toBe('snare');
    expect(pieceOfNote(60)).toBeNull();
  });

  it('encodes floats as f32 bits', () => {
    const [op, bits] = encodeCommand({ type: 'setTempo', bpm: 96.5 });
    expect(op).toBe(7);
    expect(new Float32Array(new Uint32Array([bits]).buffer)[0]).toBe(96.5);
  });
});

describe('decodeEvent', () => {
  it('decodes notes with channel and key packed together', () => {
    expect(decodeEvent(1, (2 << 8) | 40, 100, 1234)).toEqual({
      type: 'noteOn',
      channel: 2,
      key: 40,
      velocity: 100,
      frame: 1234,
    });
  });

  it('decodes chords with inversions', () => {
    // Am7 over C: root 9, quality index 11 (m7), bass 0.
    const event = decodeEvent(4, 9, (0 << 8) | 11, 0);
    expect(event).toMatchObject({ type: 'chord', chord: { name: 'Am7/C' } });
    expect(decodeEvent(4, 0xff, 0, 0)).toMatchObject({ chord: null });
  });

  it('decodes band state', () => {
    // Mirrors encodes_band_state in event.rs.
    const a = 2 | (0xff << 8) | (2 << 16) | (1 << 24);
    const b = 2 | (0b11101 << 8) | (200 << 16);
    expect(decodeEvent(7, a, b, 5)).toEqual({
      type: 'band',
      frame: 5,
      state: {
        style: 2,
        nextStyle: null,
        playing: true,
        section: 'fill',
        variation: 1,
        nextVariation: 2,
        fillPending: false,
        introPending: true,
        endingPending: true,
        follow: true,
        cued: false,
        energy: 200 / 255,
      },
    });
    expect(decodeEvent(7, a, b | (32 << 8), 5)).toMatchObject({ state: { cued: true } });
  });

  it('decodes drummer events', () => {
    const [, bits] = encodeCommand({ type: 'setTempo', bpm: -12.5 });
    expect(decodeEvent(12, bits, 0, 1)).toEqual({ type: 'timing', ms: -12.5, frame: 1 });
    expect(decodeEvent(13, 3, 0, 2)).toEqual({ type: 'countIn', hits: 3, frame: 2 });
  });

  it('decodes judged hits like event.rs', () => {
    const [, bits] = encodeCommand({ type: 'setTempo', bpm: -12.5 });
    // China (16), sixteenth 15 of bar 511.
    expect(decodeEvent(14, bits, 16 | (15 << 8) | (511 << 16), 9)).toEqual({ type: 'judged', ms: -12.5, piece: 'china', sixteenth: 15, bar: 511, frame: 9 });
    expect(decodeEvent(14, bits, 99, 9)).toBeNull();
  });

  it('decodes song positions', () => {
    expect(decodeEvent(8, 17, 0, 3)).toEqual({ type: 'songPosition', beat: 17, frame: 3 });
    expect(decodeEvent(9, 1, 1200, 4)).toEqual({ type: 'hold', holding: true, tick: 1200, frame: 4 });
    expect(decodeEvent(10, 255, 60, 0)).toEqual({ type: 'input', level: 1, key: 60, frame: 0 });
    expect(decodeEvent(10, 0, 0xff, 0)).toEqual({ type: 'input', level: 0, key: null, frame: 0 });
    const [, bits] = encodeCommand({ type: 'setTempo', bpm: 101.5 });
    expect(decodeEvent(11, bits, 0, 0)).toEqual({ type: 'tempo', bpm: 101.5, frame: 0 });
  });

  it('ignores unknown opcodes', () => {
    expect(decodeEvent(99, 0, 0, 0)).toBeNull();
  });

  it('lists chord tones', () => {
    expect(chordTones({ root: 9, quality: 11 })).toEqual(['A', 'C', 'E', 'G']);
  });

  it('names chords', () => {
    expect(chordName(0, 0, 0)).toBe('C');
    expect(chordName(10, 9, 10)).toBe('Bb7');
    expect(chordName(0, 0, 4)).toBe('C/E');
  });
});

describe('midiToCommand', () => {
  it('handles note on/off on any channel', () => {
    expect(midiToCommand([0x93, 60, 100])).toEqual({ type: 'noteOn', note: 60, velocity: 100 });
    expect(midiToCommand([0x80, 60, 64])).toEqual({ type: 'noteOff', note: 60 });
    expect(midiToCommand([0x90, 60, 0])).toEqual({ type: 'noteOff', note: 60 });
  });

  it('plays channel 10 as drums, with chokes and the hi-hat pedal', () => {
    expect(midiToCommand([0x99, 38, 120])).toEqual({ type: 'drumNote', note: 38, velocity: 120 });
    expect(midiToCommand([0x89, 38, 0])).toBeNull();
    expect(midiToCommand([0x99, 38, 0])).toBeNull();
    expect(midiToCommand([0xa9, 49, 127])).toEqual({ type: 'drumChoke', note: 49 });
    expect(midiToCommand([0xa9, 49, 0])).toBeNull();
    expect(midiToCommand([0xb9, 4, 90])).toEqual({ type: 'hiHatPedal', value: 90 });
  });

  it('maps all-notes-off and ignores the rest', () => {
    expect(midiToCommand([0xb0, 123, 0])).toEqual({ type: 'allNotesOff' });
    expect(midiToCommand([0xb0, 7, 100])).toEqual({ type: 'controlChange', cc: 7, value: 100 });
    expect(midiToCommand([0xb0, 121, 0])).toBeNull();
    expect(midiToCommand([0xf8])).toBeNull(); // clock
  });
});

describe('eventToMidi', () => {
  it('sends band notes but not the player by default', () => {
    expect(eventToMidi({ type: 'noteOn', channel: 2, key: 40, velocity: 90 })).toEqual([0x92, 40, 90]);
    expect(eventToMidi({ type: 'noteOn', channel: 0, key: 60, velocity: 90 })).toBeNull();
    expect(eventToMidi({ type: 'noteOn', channel: 0, key: 60, velocity: 90 }, { echoPlayer: true })).toEqual([
      0x90, 60, 90,
    ]);
    expect(eventToMidi({ type: 'programChange', channel: 1, program: 4 })).toEqual([0xc1, 4]);
    expect(eventToMidi({ type: 'beat', bar: 0, beat: 0 })).toBeNull();
    // The player's drums go out on channel 10 when echoed.
    expect(eventToMidi({ type: 'noteOn', channel: 14, key: 38, velocity: 90 })).toBeNull();
    expect(eventToMidi({ type: 'noteOn', channel: 14, key: 38, velocity: 90 }, { echoPlayer: true })).toEqual([0x99, 38, 90]);
  });
});
