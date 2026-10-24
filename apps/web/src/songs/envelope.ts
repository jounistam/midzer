// The level of a recording over time, for the timeline editor: as the
// analysis keeps it (`envelope` in crates/midzer-analyze), for songs imported
// before it did.

import { decodeRecording } from './analyzeAudio';
import { loadRecording } from './recordings';

/** Frames a second. */
export const ENVELOPE_FPS = 100;
const RATE = 22_050;

/** The level of 16-bit samples, `ENVELOPE_FPS` a second, from −60 dBFS (0) to 0 dBFS (255). */
export function envelopeOf(samples: Int16Array, rate = RATE): Uint8Array {
  const at = (k: number) => Math.min(samples.length, Math.floor((k * rate) / ENVELOPE_FPS));
  const out = new Uint8Array(Math.ceil((samples.length * ENVELOPE_FPS) / rate));
  for (let k = 0; k < out.length; k++) {
    let power = 0;
    const [from, to] = [at(k), at(k + 1)];
    for (let i = from; i < to; i++) power += (samples[i]! / 32768) ** 2;
    const db = 10 * Math.log10(Math.max(1e-12, power / Math.max(1, to - from)));
    out[k] = Math.round(Math.min(255, Math.max(0, ((db + 60) / 60) * 255)));
  }
  return out;
}

/** A stored recording's level, or null if it is not kept or can't be read. */
export async function recordingEnvelope(songId: string): Promise<Uint8Array | null> {
  const bytes = await loadRecording(songId);
  if (!bytes) return null;
  try {
    return envelopeOf(await decodeRecording(bytes, RATE), RATE);
  } catch {
    return null;
  }
}
