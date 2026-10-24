// Runs the recording analysis (crates/midzer-analyze, built to
// public/wasm/analyze.wasm) off the main thread, or a song heard live, or
// decodes a recording to play along with. One worker per job: it is terminated afterwards, which
// frees the memory the job needed.

interface Exports {
  memory: WebAssembly.Memory;
  mza_alloc(len: number): number;
  mza_analyze(ptr: number, len: number): number;
  mza_analyze_heard(ptr: number, len: number): number;
  mza_result_ptr(): number;
  mza_result_len(): number;
  mza_track(ptr: number, len: number, rate: number): number;
  mza_samples_ptr(): number;
  mza_samples_len(): number;
}

/**
 * Analyse the file, or with `rate` decode it as interleaved stereo at that
 * rate, or with `heard` analyse a song heard live (`bytes` are then 16-bit
 * mono samples at 22.05 kHz).
 */
export interface WorkerJob {
  bytes: ArrayBuffer;
  rate?: number;
  heard?: boolean;
}

export type WorkerReply =
  | { type: 'progress'; fraction: number; step: number }
  | { type: 'done'; json: string }
  | { type: 'samples'; samples: Int16Array<ArrayBuffer> }
  | { type: 'error'; message: string };

const post = (reply: WorkerReply) => postMessage(reply);

onmessage = async (e: MessageEvent<WorkerJob>) => {
  try {
    const env = { mza_progress: (fraction: number, step: number) => post({ type: 'progress', fraction, step }) };
    const { instance } = await WebAssembly.instantiateStreaming(fetch('/wasm/analyze.wasm'), { env });
    const x = instance.exports as unknown as Exports;
    const bytes = new Uint8Array(e.data.bytes);
    const ptr = x.mza_alloc(bytes.length);
    new Uint8Array(x.memory.buffer, ptr, bytes.length).set(bytes);
    const result = () => new TextDecoder().decode(new Uint8Array(x.memory.buffer, x.mza_result_ptr(), x.mza_result_len()));
    if (e.data.rate) {
      if (!x.mza_track(ptr, bytes.length, e.data.rate)) return post({ type: 'error', message: result() });
      const samples = new Int16Array(x.memory.buffer, x.mza_samples_ptr(), x.mza_samples_len()).slice();
      postMessage({ type: 'samples', samples } satisfies WorkerReply, { transfer: [samples.buffer] });
      return;
    }
    const ok = e.data.heard ? x.mza_analyze_heard(ptr, bytes.length) : x.mza_analyze(ptr, bytes.length);
    const text = result();
    post(ok ? { type: 'done', json: text } : { type: 'error', message: text });
  } catch (err) {
    post({ type: 'error', message: `The analysis stopped: ${err instanceof Error ? err.message : String(err)}` });
  }
};
