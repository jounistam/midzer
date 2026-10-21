// Runs the recording analysis (crates/midzer-analyze, built to
// public/wasm/analyze.wasm) off the main thread. One worker per file: it is
// terminated afterwards, which frees the memory the analysis needed.

interface Exports {
  memory: WebAssembly.Memory;
  mza_alloc(len: number): number;
  mza_analyze(ptr: number, len: number): number;
  mza_result_ptr(): number;
  mza_result_len(): number;
}

export type WorkerReply =
  | { type: 'progress'; fraction: number; step: number }
  | { type: 'done'; json: string }
  | { type: 'error'; message: string };

const post = (reply: WorkerReply) => postMessage(reply);

onmessage = async (e: MessageEvent<{ bytes: ArrayBuffer }>) => {
  try {
    const env = { mza_progress: (fraction: number, step: number) => post({ type: 'progress', fraction, step }) };
    const { instance } = await WebAssembly.instantiateStreaming(fetch('/wasm/analyze.wasm'), { env });
    const x = instance.exports as unknown as Exports;
    const bytes = new Uint8Array(e.data.bytes);
    const ptr = x.mza_alloc(bytes.length);
    new Uint8Array(x.memory.buffer, ptr, bytes.length).set(bytes);
    const ok = x.mza_analyze(ptr, bytes.length);
    const text = new TextDecoder().decode(new Uint8Array(x.memory.buffer, x.mza_result_ptr(), x.mza_result_len()));
    post(ok ? { type: 'done', json: text } : { type: 'error', message: text });
  } catch (err) {
    post({ type: 'error', message: `The analysis stopped: ${err instanceof Error ? err.message : String(err)}` });
  }
};
