// Analyses a recording where it runs best: in a worker (wasm) in the
// browser, natively in the desktop app, which can also separate the stems.

import { isNativeHost } from '@midzer/audio-web';
import type { AudioAnalysis } from './audio';
import type { WorkerReply } from './analyze.worker';

export interface AnalyzeOptions {
  /** Separate the instruments first (desktop only): slower, with a better melody and MIDI parts. */
  stems: boolean;
  onProgress: (fraction: number, step: number) => void;
  signal?: AbortSignal;
}

/** Whether stem separation is available here. */
export const canSeparateStems = isNativeHost;

export async function analyzeAudio(bytes: Uint8Array, options: AnalyzeOptions): Promise<AudioAnalysis> {
  if (isNativeHost()) {
    const { analyzeNative } = await import('@midzer/audio-web/native');
    const json = await analyzeNative(bytes, options.stems, options.onProgress, options.signal);
    return JSON.parse(json) as AudioAnalysis;
  }
  const worker = new Worker(new URL('./analyze.worker.ts', import.meta.url), { type: 'module' });
  try {
    return await new Promise<AudioAnalysis>((resolve, reject) => {
      options.signal?.addEventListener('abort', () => reject(new DOMException('Cancelled', 'AbortError')));
      worker.onerror = (e) => reject(new Error(`The analysis stopped: ${e.message || 'the worker failed'}`));
      worker.onmessage = (e: MessageEvent<WorkerReply>) => {
        const reply = e.data;
        if (reply.type === 'progress') options.onProgress(reply.fraction, reply.step);
        else if (reply.type === 'done') resolve(JSON.parse(reply.json) as AudioAnalysis);
        else reject(new Error(reply.message));
      };
      const copy = bytes.slice().buffer;
      worker.postMessage({ bytes: copy }, [copy]);
    });
  } finally {
    worker.terminate();
  }
}
