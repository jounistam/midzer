// Listens to the live source off the audio thread: the networks of
// crates/midzer-hear (built to hear.wasm) find the beats, the 1s and the
// chords. Audio arrives from the worklet through a TapRing; each result goes
// back to the main thread, in engine frames. With capture on, each song heard
// goes back too, as 16-bit samples at 22.05 kHz, to be learned.

import { TapRing } from './tap';

interface Exports {
  memory: WebAssembly.Memory;
  mzh_create(rate: number): number;
  mzh_input_ptr(l: number): number;
  mzh_push(l: number, len: number, frame: number): void;
  mzh_step(l: number): number;
  mzh_result_ptr(l: number): number;
  mzh_result_len(l: number): number;
  mzh_reset(l: number): void;
  mzh_set_capture(l: number, on: number): void;
  mzh_finish(l: number): void;
  mzh_take_song(l: number): number;
  mzh_song_ptr(l: number): number;
  mzh_song_flags(l: number): number;
}

export type HearRequest =
  | { type: 'start'; wasmUrl: string; tap: SharedArrayBuffer; sampleRate: number }
  /** Forget the song: the next audio starts afresh. */
  | { type: 'reset' }
  /** Capture the songs heard, to learn them (or stop). */
  | { type: 'capture'; on: boolean }
  /** End the song being captured here. */
  | { type: 'finish' };

export type HearReply =
  | { type: 'grid'; frame: number; period: number; one: number | null; confidence: number }
  /** Chord spans, four numbers each: see `decodeChordSpans`. */
  | { type: 'chords'; spans: Float64Array<ArrayBuffer> }
  /** A song captured: mono at 22.05 kHz; flags bit 0 heard from its start, bit 1 to its end. */
  | { type: 'song'; samples: Int16Array<ArrayBuffer>; flags: number }
  | { type: 'error'; message: string };

/** The wasm side takes at most this many samples a push (`INPUT` in ffi.rs). */
const INPUT = 16_384;
const POLL_MS = 25;

let x: Exports | null = null;
let listener = 0;
let tap: TapRing | null = null;
let capturing = false;
const block = new Float32Array(128);

const post = (reply: HearReply, transfer: Transferable[] = []) => postMessage(reply, { transfer });

/** Moves the queued audio into the listener, in runs of contiguous frames. */
function drain(x: Exports, tap: TapRing): void {
  let input = new Float32Array(x.memory.buffer, x.mzh_input_ptr(listener), INPUT);
  let len = 0;
  let start = 0;
  const flush = () => {
    if (len > 0) x.mzh_push(listener, len, start);
    len = 0;
    // A push may grow memory, detaching the view.
    input = new Float32Array(x.memory.buffer, x.mzh_input_ptr(listener), INPUT);
  };
  for (;;) {
    const n = tap.pop(block);
    if (n === 0) break;
    if (len > 0 && (tap.frame !== start + len || len + n > INPUT)) flush();
    if (len === 0) start = tap.frame;
    input.set(n === block.length ? block : block.subarray(0, n), len);
    len += n;
  }
  flush();
}

function tick(): void {
  if (!x || !tap) return;
  drain(x, tap);
  // One network run at most, then the audio that came meanwhile.
  const kind = x.mzh_step(listener);
  if (kind !== 0) {
    const r = new Float64Array(x.memory.buffer, x.mzh_result_ptr(listener), x.mzh_result_len(listener));
    if (kind === 1) {
      post({ type: 'grid', frame: r[0]!, period: r[1]!, one: r[2]! < 0 ? null : r[2]!, confidence: r[3]! });
    } else {
      const spans = r.slice();
      post({ type: 'chords', spans }, [spans.buffer]);
    }
  }
  takeSong(x);
  setTimeout(tick, kind !== 0 ? 0 : POLL_MS);
}

function takeSong(x: Exports): void {
  const len = x.mzh_take_song(listener);
  if (len === 0) return;
  const samples = new Int16Array(x.memory.buffer, x.mzh_song_ptr(listener), len).slice();
  post({ type: 'song', samples, flags: x.mzh_song_flags(listener) }, [samples.buffer]);
}

onmessage = async (e: MessageEvent<HearRequest>) => {
  const msg = e.data;
  try {
    if (msg.type === 'start') {
      const { instance } = await WebAssembly.instantiateStreaming(fetch(msg.wasmUrl), {});
      x = instance.exports as unknown as Exports;
      listener = x.mzh_create(msg.sampleRate);
      if (!listener) throw new Error('the listening networks did not load');
      x.mzh_set_capture(listener, capturing ? 1 : 0);
      tap = new TapRing(msg.tap);
      tick();
    } else if (msg.type === 'capture') {
      capturing = msg.on;
      x?.mzh_set_capture(listener, msg.on ? 1 : 0);
    } else if (msg.type === 'reset' && x) {
      tap?.clear();
      x.mzh_reset(listener);
    } else if (msg.type === 'finish' && x && tap) {
      // What is queued belongs to the song.
      drain(x, tap);
      x.mzh_finish(listener);
      takeSong(x);
    }
  } catch (err) {
    x = null;
    post({ type: 'error', message: err instanceof Error ? err.message : String(err) });
  }
};
