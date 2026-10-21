// Analyses a recording where it runs best: in a worker (wasm) in the
// browser, natively in the desktop app, which can also separate the stems.

import { isNativeHost } from '@midzer/audio-web';
import type { AudioAnalysis } from './audio';
import type { WorkerJob, WorkerReply } from './analyze.worker';

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
  const reply = await runWorker(bytes, undefined, options.onProgress, options.signal);
  if (reply.type !== 'done') throw new Error('The analysis stopped.');
  return JSON.parse(reply.json) as AudioAnalysis;
}

/**
 * Decodes a recording in a worker (the browser's engine takes samples), with
 * the same decoder as the analysis so its beat times match what is heard.
 * Interleaved stereo at `rate`.
 */
export async function decodeRecording(bytes: Uint8Array, rate: number, signal?: AbortSignal): Promise<Int16Array> {
  const reply = await runWorker(bytes, rate, () => {}, signal);
  if (reply.type !== 'samples') throw new Error('The recording could not be read.');
  return reply.samples;
}

function runWorker(
  bytes: Uint8Array,
  rate: number | undefined,
  onProgress: (fraction: number, step: number) => void,
  signal?: AbortSignal,
): Promise<WorkerReply> {
  const worker = new Worker(new URL('./analyze.worker.ts', import.meta.url), { type: 'module' });
  return new Promise<WorkerReply>((resolve, reject) => {
    signal?.addEventListener('abort', () => reject(new DOMException('Cancelled', 'AbortError')));
    worker.onerror = (e) => reject(new Error(`The analysis stopped: ${e.message || 'the worker failed'}`));
    worker.onmessage = (e: MessageEvent<WorkerReply>) => {
      const reply = e.data;
      if (reply.type === 'progress') onProgress(reply.fraction, reply.step);
      else if (reply.type === 'error') reject(new Error(reply.message));
      else resolve(reply);
    };
    const copy = bytes.slice().buffer;
    worker.postMessage({ bytes: copy, rate } satisfies WorkerJob, [copy]);
  }).finally(() => worker.terminate());
}
