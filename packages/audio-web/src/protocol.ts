// Mirror of crates/midzer-core/src/{command,event}.rs — keep opcodes in sync.

export const WORDS_PER_RECORD = 4;
export type RecordWords = [number, number, number, number];

// ---- Commands: UI → engine -------------------------------------------------

const Cmd = {
  NoteOn: 1,
  NoteOff: 2,
  SetGain: 3,
  AllNotesOff: 4,
  Play: 5,
  Stop: 6,
  SetTempo: 7,
  SetSplit: 8,
  SetMute: 9,
} as const;

export type EngineCommand =
  | { type: 'noteOn'; note: number; velocity: number }
  | { type: 'noteOff'; note: number }
  | { type: 'setGain'; gain: number }
  | { type: 'allNotesOff' }
  | { type: 'play' }
  | { type: 'stop' }
  | { type: 'setTempo'; bpm: number }
  /** Notes below `note` form chords; 0 uses the whole keyboard. */
  | { type: 'setSplit'; note: number }
  | { type: 'setMute'; channel: number; muted: boolean };

const f32 = new Float32Array(1);
const f32Bits = new Uint32Array(f32.buffer);
const floatBits = (value: number) => {
  f32[0] = value;
  return f32Bits[0]!;
};

export function encodeCommand(cmd: EngineCommand): RecordWords {
  switch (cmd.type) {
    case 'noteOn':
      return [Cmd.NoteOn, cmd.note, cmd.velocity, 0];
    case 'noteOff':
      return [Cmd.NoteOff, cmd.note, 0, 0];
    case 'setGain':
      return [Cmd.SetGain, floatBits(cmd.gain), 0, 0];
    case 'allNotesOff':
      return [Cmd.AllNotesOff, 0, 0, 0];
    case 'play':
      return [Cmd.Play, 0, 0, 0];
    case 'stop':
      return [Cmd.Stop, 0, 0, 0];
    case 'setTempo':
      return [Cmd.SetTempo, floatBits(cmd.bpm), 0, 0];
    case 'setSplit':
      return [Cmd.SetSplit, cmd.note, 0, 0];
    case 'setMute':
      return [Cmd.SetMute, cmd.channel, cmd.muted ? 1 : 0, 0];
  }
}

// ---- Events: engine → UI / MIDI out ----------------------------------------

/** Order matches `Quality::ALL` in crates/midzer-core/src/chord.rs. */
const QUALITY_SUFFIXES = [
  '', 'm', 'dim', 'aug', 'sus2', 'sus4', '5', '6', 'm6', '7', 'maj7', 'm7', 'm7b5', 'dim7', 'add9',
] as const;
/** Chord-tone intervals per quality; mirrors `Quality::intervals`. */
const QUALITY_INTERVALS: readonly (readonly number[])[] = [
  [0, 4, 7], [0, 3, 7], [0, 3, 6], [0, 4, 8], [0, 2, 7], [0, 5, 7], [0, 7], [0, 4, 7, 9],
  [0, 3, 7, 9], [0, 4, 7, 10], [0, 4, 7, 11], [0, 3, 7, 10], [0, 3, 6, 10], [0, 3, 6, 9], [0, 2, 4, 7],
];
export const NOTE_NAMES = ['C', 'C#', 'D', 'Eb', 'E', 'F', 'F#', 'G', 'Ab', 'A', 'Bb', 'B'] as const;
const NO_CHORD = 0xff;

export interface ChordInfo {
  root: number;
  bass: number;
  quality: number;
  /** Display name, e.g. "Am7" or "C/E". */
  name: string;
}

export type EngineEvent =
  | { type: 'noteOn'; channel: number; key: number; velocity: number }
  | { type: 'noteOff'; channel: number; key: number }
  | { type: 'programChange'; channel: number; program: number }
  | { type: 'chord'; chord: ChordInfo | null }
  | { type: 'beat'; bar: number; beat: number }
  | { type: 'transport'; playing: boolean };

/** An event plus the AudioContext frame at which it sounds. */
export type TimedEvent = EngineEvent & { frame: number };

/** Note names of the chord tones, root first, e.g. ["A", "C", "E", "G"]. */
export function chordTones(chord: Pick<ChordInfo, 'root' | 'quality'>): string[] {
  return (QUALITY_INTERVALS[chord.quality] ?? []).map((i) => NOTE_NAMES[(chord.root + i) % 12]!);
}

export function chordName(root: number, quality: number, bass: number): string {
  const name = `${NOTE_NAMES[root % 12]}${QUALITY_SUFFIXES[quality] ?? ''}`;
  return bass === root ? name : `${name}/${NOTE_NAMES[bass % 12]}`;
}

export function decodeEvent(op: number, a: number, b: number, frame: number): TimedEvent | null {
  switch (op) {
    case 1:
      return { type: 'noteOn', channel: a >> 8, key: a & 0xff, velocity: b, frame };
    case 2:
      return { type: 'noteOff', channel: a >> 8, key: a & 0xff, frame };
    case 3:
      return { type: 'programChange', channel: a, program: b, frame };
    case 4: {
      if (a === NO_CHORD) return { type: 'chord', chord: null, frame };
      const quality = b & 0xff;
      const bass = b >> 8;
      return { type: 'chord', chord: { root: a, bass, quality, name: chordName(a, quality, bass) }, frame };
    }
    case 5:
      return { type: 'beat', bar: a, beat: b, frame };
    case 6:
      return { type: 'transport', playing: a !== 0, frame };
    default:
      return null;
  }
}
