/// <reference path="./worklet-globals.d.ts" />
// Runs on the audio thread. Drains commands, calls into the Rust engine,
// copies its audio into the Web Audio outputs and forwards its events to the
// main thread. Must not allocate in process().

import { RecordRing } from './ring';
import { TapRing } from './tap';

export const PROCESSOR_NAME = 'midzer-engine';

export interface ProcessorOptions {
  module: WebAssembly.Module;
  /** UI → engine commands; null when not cross-origin isolated. */
  commands: SharedArrayBuffer | null;
  /** Engine → UI events; null when not cross-origin isolated. */
  events: SharedArrayBuffer | null;
  /** The live source for the listening worker (see tap.ts); null when not cross-origin isolated. */
  tap: SharedArrayBuffer | null;
}

/** Messages on the node's port, both directions. */
export type PortMessage =
  | { type: 'command'; words: number[] }
  | { type: 'soundfont'; bytes: ArrayBuffer }
  | { type: 'soundfontLoaded'; ok: boolean }
  /** Interleaved stereo 16-bit samples at the context's rate; null frees the track. */
  | { type: 'track'; main: ArrayBuffer | null; drums: ArrayBuffer | null }
  | { type: 'trackLoaded'; frames: number }
  | { type: 'events'; words: Uint32Array }
  | { type: 'overflow'; dropped: number };

interface MidzerExports {
  memory: WebAssembly.Memory;
  mz_create(sampleRate: number): number;
  mz_alloc(len: number): number;
  mz_alloc_samples(len: number): number;
  mz_load_track(host: number, main: number, drums: number, len: number): void;
  mz_free(ptr: number, len: number): void;
  mz_load_soundfont(host: number, ptr: number, len: number): number;
  mz_left_ptr(host: number): number;
  mz_right_ptr(host: number): number;
  mz_events_ptr(host: number): number;
  mz_input_ptr(host: number): number;
  mz_listen(host: number, frames: number): void;
  mz_live_ptr(host: number): number;
  mz_listen_live(host: number, frames: number): void;
  mz_now(host: number): number;
  mz_command(host: number, w0: number, w1: number, w2: number, w3: number): number;
  mz_process(host: number, frames: number): number;
}

const BLOCK = 128;
const MAX_EVENTS = 512;

class MidzerProcessor extends AudioWorkletProcessor {
  private readonly wasm: MidzerExports;
  private readonly host: number;
  private readonly commands: RecordRing | null;
  private readonly events: RecordRing | null;
  private readonly tap: TapRing | null;
  private readonly record = new Uint32Array(4);
  private left!: Float32Array;
  private right!: Float32Array;
  private eventWords!: Uint32Array;
  private input!: Float32Array;
  private live!: Float32Array;
  private dropped = 0;

  constructor(options: AudioWorkletNodeOptions) {
    super(options);
    const opts = options.processorOptions as ProcessorOptions;
    this.wasm = new WebAssembly.Instance(opts.module, {}).exports as unknown as MidzerExports;
    this.host = this.wasm.mz_create(sampleRate);
    this.commands = opts.commands ? new RecordRing(opts.commands) : null;
    this.events = opts.events ? new RecordRing(opts.events) : null;
    this.tap = opts.tap ? new TapRing(opts.tap) : null;
    this.refreshViews();
    this.port.onmessage = (e: MessageEvent<PortMessage>) => this.onMessage(e.data);
  }

  private onMessage(msg: PortMessage) {
    const { wasm, host } = this;
    if (msg.type === 'command') {
      const [w0 = 0, w1 = 0, w2 = 0, w3 = 0] = msg.words;
      wasm.mz_command(host, w0, w1, w2, w3);
    } else if (msg.type === 'soundfont') {
      // Parsing blocks this thread for a moment; the UI loads it before playback.
      const len = msg.bytes.byteLength;
      const ptr = wasm.mz_alloc(len);
      new Uint8Array(wasm.memory.buffer, ptr, len).set(new Uint8Array(msg.bytes));
      const ok = wasm.mz_load_soundfont(host, ptr, len) === 1;
      wasm.mz_free(ptr, len);
      this.refreshViews();
      this.port.postMessage({ type: 'soundfontLoaded', ok } satisfies PortMessage);
    } else if (msg.type === 'track') {
      // Copying a few minutes of audio blocks this thread briefly; it happens while stopped.
      const len = msg.main ? msg.main.byteLength / 2 : 0;
      const copy = (bytes: ArrayBuffer | null) => {
        if (!bytes || len === 0) return 0;
        const ptr = wasm.mz_alloc_samples(len);
        new Int16Array(wasm.memory.buffer, ptr, len).set(new Int16Array(bytes, 0, len));
        return ptr;
      };
      const main = copy(msg.main);
      const drums = copy(msg.drums);
      wasm.mz_load_track(host, main, drums, len);
      this.refreshViews();
      this.port.postMessage({ type: 'trackLoaded', frames: len / 2 } satisfies PortMessage);
    }
  }

  process(inputs: Float32Array[][], outputs: Float32Array[][]): boolean {
    const { wasm, host, record } = this;
    if (this.commands) {
      while (this.commands.pop(record)) {
        wasm.mz_command(host, record[0]!, record[1]!, record[2]!, record[3]!);
      }
    }

    const out = outputs[0];
    const frames = out?.[0]?.length ?? BLOCK;
    // The microphone, when connected; an unconnected input has no channels.
    const mic = inputs[0]?.[0];
    if (mic) {
      if (this.input.buffer !== wasm.memory.buffer) this.refreshViews();
      this.input.set(mic.length === BLOCK ? mic : mic.subarray(0, Math.min(mic.length, BLOCK)));
      wasm.mz_listen(host, Math.min(mic.length, BLOCK));
    }
    // The live source to play along with, on the second input.
    const live = inputs[1]?.[0];
    if (live) {
      if (this.live.buffer !== wasm.memory.buffer) this.refreshViews();
      this.live.set(live.length === BLOCK ? live : live.subarray(0, Math.min(live.length, BLOCK)));
      const n = Math.min(live.length, BLOCK);
      const tap = this.tap;
      if (tap?.enabled) {
        const now = wasm.mz_now(host);
        tap.frameOffset = currentFrame - now;
        tap.push(live, n, now);
      }
      wasm.mz_listen_live(host, n);
    }
    const count = wasm.mz_process(host, frames);

    // Growing wasm memory detaches old views; re-create them only then.
    if (this.left.buffer !== wasm.memory.buffer) this.refreshViews();
    if (out && out.length > 0) {
      out[0]!.set(frames === BLOCK ? this.left : this.left.subarray(0, frames));
      out[1]?.set(frames === BLOCK ? this.right : this.right.subarray(0, frames));
    }
    if (count > 0) this.forwardEvents(count);
    return true;
  }

  /** Sends events with absolute AudioContext frames instead of block offsets. */
  private forwardEvents(count: number) {
    const w = this.eventWords;
    if (this.events) {
      for (let i = 0; i < count * 4; i += 4) {
        if (!this.events.push4(w[i]!, w[i + 1]!, w[i + 2]!, currentFrame + w[i + 3]!)) this.dropped++;
      }
      if (this.dropped > 0 && currentFrame % (BLOCK * 375) < BLOCK) {
        this.port.postMessage({ type: 'overflow', dropped: this.dropped } satisfies PortMessage);
        this.dropped = 0;
      }
    } else {
      // Fallback without shared memory; allocates, but only in that mode.
      const words = w.slice(0, count * 4);
      for (let i = 3; i < words.length; i += 4) words[i]! += currentFrame;
      this.port.postMessage({ type: 'events', words } satisfies PortMessage, [words.buffer]);
    }
  }

  private refreshViews() {
    const { memory } = this.wasm;
    this.left = new Float32Array(memory.buffer, this.wasm.mz_left_ptr(this.host), BLOCK);
    this.right = new Float32Array(memory.buffer, this.wasm.mz_right_ptr(this.host), BLOCK);
    this.eventWords = new Uint32Array(memory.buffer, this.wasm.mz_events_ptr(this.host), MAX_EVENTS * 4);
    this.input = new Float32Array(memory.buffer, this.wasm.mz_input_ptr(this.host), BLOCK);
    this.live = new Float32Array(memory.buffer, this.wasm.mz_live_ptr(this.host), BLOCK);
  }
}

registerProcessor(PROCESSOR_NAME, MidzerProcessor);
