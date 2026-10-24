// Standard MIDI Files: a small reader (for import) and writer (for exporting
// performances). Only what midzer needs: notes, programs, tempo, meter and
// track names. Other events are skipped on read.

export interface SmfNote {
  tick: number;
  duration: number;
  channel: number;
  key: number;
  velocity: number;
}

export interface SmfTrack {
  name: string;
  notes: SmfNote[];
  programs: { tick: number; channel: number; program: number }[];
}

export interface Smf {
  /** Ticks per quarter note. */
  ppq: number;
  tracks: SmfTrack[];
  /** Microseconds per quarter note, sorted by tick. */
  tempos: { tick: number; usPerQuarter: number }[];
  meters: { tick: number; numerator: number; denominator: number }[];
}

class Reader {
  pos = 0;
  constructor(
    private readonly bytes: Uint8Array,
    private end = bytes.length,
  ) {}

  get done() {
    return this.pos >= this.end;
  }
  u8() {
    if (this.pos >= this.end) throw new Error('The MIDI file ends too early.');
    return this.bytes[this.pos++]!;
  }
  u16() {
    return (this.u8() << 8) | this.u8();
  }
  u32() {
    return ((this.u16() << 16) | this.u16()) >>> 0;
  }
  text(len: number) {
    const s = String.fromCharCode(...this.bytes.subarray(this.pos, this.pos + len));
    this.skip(len);
    return s;
  }
  skip(len: number) {
    this.pos = Math.min(this.end, this.pos + len);
  }
  varLen() {
    let value = 0;
    for (let i = 0; i < 4; i++) {
      const b = this.u8();
      value = (value << 7) | (b & 0x7f);
      if (!(b & 0x80)) return value;
    }
    throw new Error('Bad variable-length number in the MIDI file.');
  }
  sub(len: number) {
    const r = new Reader(this.bytes, Math.min(this.end, this.pos + len));
    r.pos = this.pos;
    this.skip(len);
    return r;
  }
}

/** Parses a Standard MIDI File (format 0 or 1). Throws with a readable message. */
export function readSmf(data: ArrayBuffer | Uint8Array): Smf {
  const r = new Reader(data instanceof Uint8Array ? data : new Uint8Array(data));
  if (r.text(4) !== 'MThd') throw new Error('This is not a MIDI file.');
  const headerLen = r.u32();
  const head = r.sub(headerLen);
  head.u16(); // format
  const trackCount = head.u16();
  const division = head.u16();
  if (division & 0x8000) throw new Error('MIDI files timed in SMPTE frames are not supported.');

  const smf: Smf = { ppq: division || 480, tracks: [], tempos: [], meters: [] };
  for (let t = 0; t < trackCount && !r.done; t++) {
    const id = r.text(4);
    const len = r.u32();
    const chunk = r.sub(len);
    if (id === 'MTrk') smf.tracks.push(readTrack(chunk, smf));
  }
  smf.tempos.sort((a, b) => a.tick - b.tick);
  smf.meters.sort((a, b) => a.tick - b.tick);
  return smf;
}

function readTrack(r: Reader, smf: Smf): SmfTrack {
  const track: SmfTrack = { name: '', notes: [], programs: [] };
  const open = new Map<number, SmfNote[]>(); // channel << 8 | key → started notes
  let tick = 0;
  let status = 0;

  const noteOff = (channel: number, key: number) => {
    const started = open.get((channel << 8) | key);
    const note = started?.shift();
    if (note) note.duration = tick - note.tick;
  };

  while (!r.done) {
    tick += r.varLen();
    let byte = r.u8();
    if (byte === 0xff) {
      const type = r.u8();
      const len = r.varLen();
      const meta = r.sub(len);
      if (type === 0x03 && !track.name) track.name = meta.text(len).trim();
      else if (type === 0x51 && len === 3) smf.tempos.push({ tick, usPerQuarter: (meta.u8() << 16) | meta.u16() });
      else if (type === 0x58 && len >= 2) smf.meters.push({ tick, numerator: meta.u8(), denominator: 2 ** meta.u8() });
      else if (type === 0x2f) break;
      continue;
    }
    if (byte === 0xf0 || byte === 0xf7) {
      r.skip(r.varLen());
      continue;
    }
    if (byte & 0x80) {
      status = byte;
      byte = r.u8();
    } else if (!status) {
      throw new Error('Bad running status in the MIDI file.');
    }
    const kind = status & 0xf0;
    const channel = status & 0x0f;
    const d1 = byte;
    const d2 = kind === 0xc0 || kind === 0xd0 ? 0 : r.u8();
    if (kind === 0x90 && d2 > 0) {
      const note: SmfNote = { tick, duration: 0, channel, key: d1, velocity: d2 };
      track.notes.push(note);
      const k = (channel << 8) | d1;
      open.set(k, [...(open.get(k) ?? []), note]);
    } else if (kind === 0x80 || kind === 0x90) {
      noteOff(channel, d1);
    } else if (kind === 0xc0) {
      track.programs.push({ tick, channel, program: d1 });
    }
  }
  // Notes never released end with the track.
  for (const notes of open.values()) for (const n of notes) n.duration = Math.max(1, tick - n.tick);
  return track;
}

// ---- Writing ------------------------------------------------------------------

/** A track to write: raw channel or meta events at absolute ticks. */
export interface TrackEvents {
  name: string;
  /** `[tick, ...bytes]`; meta events are `[tick, 0xff, type, ...data]` without the length. */
  events: number[][];
}

function varLen(value: number): number[] {
  const out = [value & 0x7f];
  while ((value >>= 7) > 0) out.unshift((value & 0x7f) | 0x80);
  return out;
}

/** Writes a format 1 file. Events in each track are sorted by tick; ties keep their order. */
export function writeSmf(ppq: number, tracks: TrackEvents[]): Uint8Array {
  const bytes: number[] = [];
  const u32 = (v: number) => bytes.push((v >>> 24) & 0xff, (v >>> 16) & 0xff, (v >>> 8) & 0xff, v & 0xff);
  bytes.push(...[...'MThd'].map((c) => c.charCodeAt(0)));
  u32(6);
  bytes.push(0, 1, (tracks.length >> 8) & 0xff, tracks.length & 0xff, (ppq >> 8) & 0xff, ppq & 0xff);

  for (const track of tracks) {
    const body: number[] = [];
    const name = [...track.name].map((c) => c.charCodeAt(0) & 0x7f);
    body.push(0, 0xff, 0x03, ...varLen(name.length), ...name);
    const sorted = track.events.map((e, i) => ({ e, i })).sort((a, b) => a.e[0]! - b.e[0]! || a.i - b.i);
    let last = 0;
    for (const { e } of sorted) {
      const tick = Math.max(last, Math.round(e[0]!));
      body.push(...varLen(tick - last));
      last = tick;
      if (e[1] === 0xff) body.push(0xff, e[2]!, ...varLen(e.length - 3), ...e.slice(3));
      else body.push(...e.slice(1));
    }
    body.push(0, 0xff, 0x2f, 0);
    bytes.push(...[...'MTrk'].map((c) => c.charCodeAt(0)));
    u32(body.length);
    for (const b of body) bytes.push(b);
  }
  return new Uint8Array(bytes);
}

/** A time signature meta event: `beats` quarter notes a bar. */
export function meterEvent(tick: number, beats: number): number[] {
  return [tick, 0xff, 0x58, beats, 2, 24, 8];
}

/** A tempo meta event. */
export function tempoEvent(tick: number, bpm: number): number[] {
  const us = Math.round(60_000_000 / bpm);
  return [tick, 0xff, 0x51, (us >> 16) & 0xff, (us >> 8) & 0xff, us & 0xff];
}
