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
  SetStyle: 10,
  Section: 11,
  SetFollow: 12,
  SetEnergy: 13,
  SetIdleMode: 14,
  SetLatency: 15,
  SetClick: 16,
} as const;

/** Built-in styles in engine order; mirrors `styles()` in crates/midzer-core/src/style.rs. */
export const STYLES = [
  { name: 'Pop', bpm: 100, blurb: 'Straight 8ths, warm piano' },
  { name: 'Rock', bpm: 120, blurb: 'Driving drums, crunchy guitar' },
  { name: 'Funk', bpm: 104, blurb: 'Tight 16ths, slap bass, clav' },
  { name: 'Bossa', bpm: 140, blurb: 'Brushes, nylon guitar, clave' },
  { name: 'Ballad', bpm: 72, blurb: 'Piano arpeggios, soft strings' },
  { name: 'Blues', bpm: 96, blurb: 'Shuffle, boogie bass, organ' },
] as const;

/** Section requests; mirrors `request_code` in command.rs. */
export type SectionRequest = 'intro' | 'A' | 'B' | 'C' | 'fill' | 'ending';
const SECTION_REQUESTS: readonly SectionRequest[] = ['intro', 'A', 'B', 'C', 'fill', 'ending'];

/** What the band does when the player stops; mirrors `IDLE_MODES` in command.rs. */
export type IdleMode = 'keepPlaying' | 'rest' | 'end';
const IDLE_MODES: readonly IdleMode[] = ['keepPlaying', 'rest', 'end'];

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
  | { type: 'setMute'; channel: number; muted: boolean }
  /** While playing, the style changes on the next bar line. */
  | { type: 'setStyle'; index: number }
  | { type: 'section'; request: SectionRequest }
  /** Whether energy and variation follow the player's touch. */
  | { type: 'setFollow'; on: boolean }
  /** Energy 0..1, used while not following. */
  | { type: 'setEnergy'; energy: number }
  | { type: 'setIdleMode'; mode: IdleMode }
  /** How late (ms) the player's notes arrive after the beat they meant. */
  | { type: 'setLatency'; ms: number }
  /** A click on every beat instead of the band, for calibration. */
  | { type: 'setClick'; on: boolean };

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
    case 'setStyle':
      return [Cmd.SetStyle, cmd.index, 0, 0];
    case 'section':
      return [Cmd.Section, SECTION_REQUESTS.indexOf(cmd.request), 0, 0];
    case 'setFollow':
      return [Cmd.SetFollow, cmd.on ? 1 : 0, 0, 0];
    case 'setEnergy':
      return [Cmd.SetEnergy, floatBits(cmd.energy), 0, 0];
    case 'setIdleMode':
      return [Cmd.SetIdleMode, IDLE_MODES.indexOf(cmd.mode), 0, 0];
    case 'setLatency':
      return [Cmd.SetLatency, floatBits(cmd.ms), 0, 0];
    case 'setClick':
      return [Cmd.SetClick, cmd.on ? 1 : 0, 0, 0];
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

/** Mirrors `Section::ALL` in crates/midzer-core/src/conductor.rs. */
export type Section = 'intro' | 'main' | 'fill' | 'ending' | 'rest';
const SECTIONS: readonly Section[] = ['intro', 'main', 'fill', 'ending', 'rest'];
export const VARIATION_NAMES = ['A', 'B', 'C'] as const;

/** Mirrors `BandState` in crates/midzer-core/src/event.rs. */
export interface BandState {
  style: number;
  /** A style change queued for the next bar line. */
  nextStyle: number | null;
  playing: boolean;
  section: Section;
  variation: number;
  /** The variation after the current intro, fill or rest. */
  nextVariation: number;
  /** Queued for the next beat (fill) or bar line (intro, ending). */
  fillPending: boolean;
  introPending: boolean;
  endingPending: boolean;
  follow: boolean;
  /** 0..1 */
  energy: number;
}

export type EngineEvent =
  | { type: 'noteOn'; channel: number; key: number; velocity: number }
  | { type: 'noteOff'; channel: number; key: number }
  | { type: 'programChange'; channel: number; program: number }
  | { type: 'chord'; chord: ChordInfo | null }
  | { type: 'beat'; bar: number; beat: number }
  | { type: 'transport'; playing: boolean }
  | { type: 'band'; state: BandState };

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
    case 7: {
      const nextStyle = (a >> 8) & 0xff;
      const flags = (b >> 8) & 0xff;
      const state: BandState = {
        style: a & 0xff,
        nextStyle: nextStyle === 0xff ? null : nextStyle,
        section: SECTIONS[(a >> 16) & 0xff] ?? 'main',
        variation: a >>> 24,
        nextVariation: b & 0xff,
        playing: (flags & 1) !== 0,
        fillPending: (flags & 2) !== 0,
        endingPending: (flags & 4) !== 0,
        follow: (flags & 8) !== 0,
        introPending: (flags & 16) !== 0,
        energy: ((b >> 16) & 0xff) / 255,
      };
      return { type: 'band', state, frame };
    }
    default:
      return null;
  }
}
