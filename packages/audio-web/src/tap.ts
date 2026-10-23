// The live source's audio, block by block, from the worklet to the listening
// worker (hear.worker.ts). A lock-free single-producer/single-consumer ring
// over a SharedArrayBuffer, like RecordRing: neither side allocates or blocks.
//
// Each slot holds one block of up to 128 samples with its length and the
// engine frame of its first sample. The header also says whether the
// listener wants audio at all, and how far the AudioContext's frames are
// ahead of the engine's (to time what it hears).

const BLOCK = 128;
/** Samples, then the length and the frame's low and high 32 bits. */
const SLOT = BLOCK + 4;
const HEADER_WORDS = 4;
const WRITE = 0;
const READ = 1;
const ENABLED = 2;
/** AudioContext frame minus engine frame, constant once the engine runs. */
const OFFSET = 3;

export class TapRing {
  private readonly header: Int32Array;
  private readonly samples: Float32Array;
  private readonly words: Uint32Array;
  private readonly mask: number;
  /** The engine frame of the block {@link pop} last returned. */
  frame = 0;

  /** About 11 s of 48 kHz audio by default: room for the worker to be busy. */
  static create(blocks = 4096): TapRing {
    if (blocks <= 0 || (blocks & (blocks - 1)) !== 0) throw new RangeError('blocks must be a power of two');
    return new TapRing(new SharedArrayBuffer(HEADER_WORDS * 4 + blocks * SLOT * 4));
  }

  constructor(readonly buffer: SharedArrayBuffer) {
    this.header = new Int32Array(buffer, 0, HEADER_WORDS);
    this.samples = new Float32Array(buffer, HEADER_WORDS * 4);
    this.words = new Uint32Array(buffer, HEADER_WORDS * 4);
    this.mask = this.samples.length / SLOT - 1;
  }

  get enabled(): boolean {
    return Atomics.load(this.header, ENABLED) !== 0;
  }

  set enabled(on: boolean) {
    Atomics.store(this.header, ENABLED, on ? 1 : 0);
  }

  get frameOffset(): number {
    return Atomics.load(this.header, OFFSET);
  }

  set frameOffset(frames: number) {
    Atomics.store(this.header, OFFSET, frames | 0);
  }

  /** Copies `len` samples of `input` as a block starting at engine `frame`; false if full. */
  push(input: Float32Array, len: number, frame: number): boolean {
    const write = Atomics.load(this.header, WRITE) >>> 0;
    const read = Atomics.load(this.header, READ) >>> 0;
    if (((write - read) >>> 0) > this.mask) return false;
    const base = (write & this.mask) * SLOT;
    const n = Math.min(len, BLOCK);
    for (let i = 0; i < n; i++) this.samples[base + i] = input[i]!;
    this.words[base + BLOCK] = n;
    this.words[base + BLOCK + 1] = frame >>> 0;
    this.words[base + BLOCK + 2] = Math.floor(frame / 2 ** 32);
    Atomics.store(this.header, WRITE, (write + 1) | 0);
    return true;
  }

  /** Copies the next block into `out` (128 long) and returns its length, or 0 if empty; its frame is in {@link frame}. */
  pop(out: Float32Array): number {
    const read = Atomics.load(this.header, READ) >>> 0;
    const write = Atomics.load(this.header, WRITE) >>> 0;
    if (read === write) return 0;
    const base = (read & this.mask) * SLOT;
    const n = this.words[base + BLOCK]!;
    out.set(this.samples.subarray(base, base + n));
    this.frame = this.words[base + BLOCK + 1]! + this.words[base + BLOCK + 2]! * 2 ** 32;
    Atomics.store(this.header, READ, (read + 1) | 0);
    return n;
  }

  /** Drops everything queued (consumer side). */
  clear(): void {
    Atomics.store(this.header, READ, Atomics.load(this.header, WRITE));
  }
}
