import { describe, expect, it } from 'vitest';
import { eventToMidi, midiToCommand } from './midi';
import { chordName, chordTones, decodeEvent, encodeCommand } from './protocol';

describe('encodeCommand', () => {
  it('encodes sections and modes like command.rs', () => {
    expect(encodeCommand({ type: 'section', request: 'C' })).toEqual([11, 3, 0, 0]);
    expect(encodeCommand({ type: 'section', request: 'ending' })).toEqual([11, 5, 0, 0]);
    expect(encodeCommand({ type: 'setIdleMode', mode: 'end' })).toEqual([14, 2, 0, 0]);
    expect(encodeCommand({ type: 'setStyle', index: 4 })).toEqual([10, 4, 0, 0]);
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
        energy: 200 / 255,
      },
    });
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

  it('maps all-notes-off and ignores the rest', () => {
    expect(midiToCommand([0xb0, 123, 0])).toEqual({ type: 'allNotesOff' });
    expect(midiToCommand([0xb0, 7, 100])).toBeNull();
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
  });
});
