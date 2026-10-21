// The engine in the desktop app: it runs on a native audio thread
// (crates/midzer-native) and is reached through Tauri commands and one update
// channel (apps/desktop/src-tauri/src/lib.rs). It offers the same EngineHandle
// as the AudioWorklet, plus native MIDI, since desktop webviews lack Web MIDI.

import { Channel, invoke } from '@tauri-apps/api/core';
import type { EngineHandle, EventListener } from './engine';
import { decodeEvent, encodeCommand, type EngineCommand } from './protocol';

export { isNativeHost } from './engine';

export interface MidiPort {
  id: string;
  name: string;
}

export interface MidiPorts {
  inputs: MidiPort[];
  outputs: MidiPort[];
}

/** MIDI handled natively: inputs play straight into the engine, the band goes out natively. */
export interface NativeMidi {
  /** Calls `listener` now and whenever devices come or go. Returns an unsubscribe function. */
  onPorts(listener: (ports: MidiPorts) => void): () => void;
  /** Calls `listener` with each command a MIDI input sent; the engine already has it. */
  onInput(listener: (cmd: EngineCommand) => void): () => void;
  setOutput(id: string | null): Promise<void>;
}

export type NativeEngineHandle = EngineHandle & { readonly midi: NativeMidi };

type Update =
  | { type: 'events'; words: number[]; heardFrame: number }
  | { type: 'input'; words: number[] }
  | ({ type: 'ports' } & MidiPorts);

interface Started {
  sampleRate: number;
  outputLatencyMs: number;
}

/** Decodes the player commands MIDI input produces (`Command::from_midi`). */
export function decodeInputCommand(op: number, a: number, b: number): EngineCommand | null {
  switch (op) {
    case 1:
      return { type: 'noteOn', note: a, velocity: b };
    case 2:
      return { type: 'noteOff', note: a };
    case 4:
      return { type: 'allNotesOff' };
    case 26:
      return { type: 'drumNote', note: a, velocity: b };
    case 30:
      return { type: 'hiHatPedal', value: a };
    case 31:
      return { type: 'drumChoke', note: a };
    default:
      return null;
  }
}

/** Maps a u32 engine frame to the performance.now() time it is heard, given the frame heard now. */
export function frameTime(frame: number, heardFrame: number, sampleRate: number, nowMs: number): number {
  const unwrapped = frame + Math.round((heardFrame - frame) / 2 ** 32) * 2 ** 32;
  return nowMs + ((unwrapped - heardFrame) / sampleRate) * 1000;
}

export async function startNativeEngine(): Promise<NativeEngineHandle> {
  const listeners = new Set<EventListener>();
  const portListeners = new Set<(ports: MidiPorts) => void>();
  const inputListeners = new Set<(cmd: EngineCommand) => void>();
  let ports: MidiPorts = { inputs: [], outputs: [] };
  let sampleRate = 48_000;

  const channel = new Channel<Update>();
  channel.onmessage = (update) => {
    if (update.type === 'events') {
      const now = performance.now();
      const w = update.words;
      for (let i = 0; i + 3 < w.length; i += 4) {
        const event = decodeEvent(w[i]!, w[i + 1]!, w[i + 2]!, w[i + 3]!);
        if (!event) continue;
        const timeMs = frameTime(event.frame, update.heardFrame, sampleRate, now);
        for (const listener of listeners) listener(event, timeMs);
      }
    } else if (update.type === 'input') {
      const w = update.words;
      for (let i = 0; i + 3 < w.length; i += 4) {
        const cmd = decodeInputCommand(w[i]!, w[i + 1]!, w[i + 2]!);
        if (cmd) for (const listener of inputListeners) listener(cmd);
      }
    } else {
      ports = { inputs: update.inputs, outputs: update.outputs };
      for (const listener of portListeners) listener(ports);
    }
  };

  const started = await invoke<Started>('engine_start', { onUpdate: channel });
  sampleRate = started.sampleRate;

  // Commands sent together (a whole song chart) travel in one call.
  let queued: number[] = [];
  const flush = () => {
    const words = queued;
    queued = [];
    invoke('engine_send', { words }).catch((e: unknown) => console.warn('midzer: engine_send failed', e));
  };

  return {
    sampleRate,
    outputLatencyMs: Math.round(started.outputLatencyMs),
    transport: 'native',
    send(cmd) {
      if (queued.length === 0) queueMicrotask(flush);
      queued.push(...encodeCommand(cmd));
    },
    onEvent(listener) {
      listeners.add(listener);
      return () => listeners.delete(listener);
    },
    async loadSoundFont(bytes) {
      try {
        await invoke('engine_load_soundfont', new Uint8Array(bytes));
        return true;
      } catch (e) {
        console.warn('midzer: SoundFont rejected', e);
        return false;
      }
    },
    async openMic() {
      // Echo cancellation is a browser feature; natively the headphones hint is all there is.
      await invoke('mic_open').catch((e: unknown) => {
        throw new Error(String(e));
      });
      return () => void invoke('mic_close');
    },
    async close() {
      listeners.clear();
      channel.onmessage = () => {};
    },
    midi: {
      onPorts(listener) {
        portListeners.add(listener);
        listener(ports);
        return () => portListeners.delete(listener);
      },
      onInput(listener) {
        inputListeners.add(listener);
        return () => inputListeners.delete(listener);
      },
      async setOutput(id) {
        await invoke('midi_set_output', { id });
      },
    },
  };
}

/**
 * Analyses a recording in the desktop app (crates/midzer-analyze, or with
 * `stems` crates/midzer-stems) and returns the analysis as JSON.
 */
export async function analyzeNative(
  bytes: Uint8Array,
  stems: boolean,
  onProgress: (fraction: number, step: number) => void,
  signal?: AbortSignal,
): Promise<string> {
  await invoke('audio_load', bytes);
  const channel = new Channel<{ fraction: number; step: number }>();
  channel.onmessage = (p) => onProgress(p.fraction, p.step);
  const cancel = () => void invoke('audio_cancel');
  signal?.addEventListener('abort', cancel);
  try {
    return await invoke<string>('audio_analyze', { stems, onProgress: channel });
  } catch (e) {
    throw new Error(String(e));
  } finally {
    signal?.removeEventListener('abort', cancel);
  }
}
