import { WORDS_PER_RECORD, type RecordWords } from './protocol';

const HEADER_BYTES = 8; // [writeIndex, readIndex] as Int32
const RECORD_BYTES = WORDS_PER_RECORD * 4;

/**
 * Lock-free single-producer/single-consumer ring of fixed-size 4-word records
 * over a SharedArrayBuffer. One side pushes, the other pops;
 * neither side allocates or blocks.
 *
 * Indices are free-running 32-bit counters, so `write - read` is the number
 * of queued records even after they wrap.
 */
export class RecordRing {
  private readonly header: Int32Array;
  private readonly data: Uint32Array;
  private readonly mask: number;

  static create(capacity = 256): RecordRing {
    if (capacity <= 0 || (capacity & (capacity - 1)) !== 0) {
      throw new RangeError('capacity must be a power of two');
    }
    return new RecordRing(new SharedArrayBuffer(HEADER_BYTES + capacity * RECORD_BYTES));
  }

  constructor(readonly buffer: SharedArrayBuffer) {
    this.header = new Int32Array(buffer, 0, 2);
    this.data = new Uint32Array(buffer, HEADER_BYTES);
    this.mask = this.data.length / WORDS_PER_RECORD - 1;
  }

  get capacity(): number {
    return this.mask + 1;
  }

  /** Returns false (dropping the record) if the ring is full. */
  push(words: RecordWords): boolean {
    return this.push4(words[0], words[1], words[2], words[3]);
  }

  /** Allocation-free variant of {@link push} for the audio thread. */
  push4(w0: number, w1: number, w2: number, w3: number): boolean {
    const write = Atomics.load(this.header, 0) >>> 0;
    const read = Atomics.load(this.header, 1) >>> 0;
    if (((write - read) >>> 0) >= this.capacity) return false;
    const start = (write & this.mask) * WORDS_PER_RECORD;
    const { data } = this;
    data[start] = w0;
    data[start + 1] = w1;
    data[start + 2] = w2;
    data[start + 3] = w3;
    Atomics.store(this.header, 0, (write + 1) | 0);
    return true;
  }

  /** Copies the next record into `out`. Returns false if the ring is empty. */
  pop(out: Uint32Array): boolean {
    const read = Atomics.load(this.header, 1) >>> 0;
    const write = Atomics.load(this.header, 0) >>> 0;
    if (read === write) return false;
    const start = (read & this.mask) * WORDS_PER_RECORD;
    out.set(this.data.subarray(start, start + WORDS_PER_RECORD));
    Atomics.store(this.header, 1, (read + 1) | 0);
    return true;
  }
}
