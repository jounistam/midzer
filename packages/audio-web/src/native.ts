// The engine in the desktop app: it runs on a native audio thread
// (crates/midzer-native) and is reached through Tauri commands and one update
// channel (apps/desktop/src-tauri/src/lib.rs). It offers the same EngineHandle
// as the AudioWorklet, plus native MIDI, since desktop webviews lack Web MIDI.

import { Channel, invoke } from '@tauri-apps/api/core';
import {
  decodeChordSpans,
  liveSongOf,
  type EngineHandle,
  type EventListener,
  type LiveChordListener,
  type LiveDevice,
  type LiveSongListener,
  type TrackLoaded,
} from './engine';
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
  | ({ type: 'ports' } & MidiPorts)
  /** What the listener heard in the live source; see `decodeChordSpans`. */
  | { type: 'chords'; spans: number[]; heardFrame: number }
  /** A song captured from the live source, kept natively until `analyzeLiveSong`; flags as in `liveSongOf`. */
  | { type: 'liveSong'; seconds: number; flags: number };

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
    case 36:
      return { type: 'controlChange', cc: a, value: b };
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
  const chordListeners = new Set<LiveChordListener>();
  const songListeners = new Set<LiveSongListener>();
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
    } else if (update.type === 'chords') {
      const now = performance.now();
      const spans = decodeChordSpans(update.spans, (frame) => frameTime(frame, update.heardFrame, sampleRate, now));
      for (const listener of chordListeners) listener(spans);
    } else if (update.type === 'liveSong') {
      const song = liveSongOf(null, update.seconds, update.flags);
      for (const listener of songListeners) listener(song);
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
    onLiveChords(listener) {
      chordListeners.add(listener);
      return () => chordListeners.delete(listener);
    },
    onLiveSong(listener) {
      songListeners.add(listener);
      return () => songListeners.delete(listener);
    },
    canLearnSongs: true,
    finishLiveSong() {
      void invoke('live_song_finish');
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
    async loadTrack(source) {
      if (!source) {
        await invoke('track_unload');
        return null;
      }
      if (source.kind !== 'file') throw new Error('The desktop engine decodes the file itself.');
      if (source.stemsDir) {
        try {
          return await invoke<TrackLoaded>('track_load_stems', { dir: source.stemsDir });
        } catch (e) {
          console.warn('midzer: stems unavailable, playing the mix', e);
        }
      }
      try {
        return await invoke<TrackLoaded>('track_load', source.bytes);
      } catch (e) {
        throw new Error(String(e));
      }
    },
    async openMic() {
      // Echo cancellation is a browser feature; natively the headphones hint is all there is.
      await invoke('mic_open').catch((e: unknown) => {
        throw new Error(String(e));
      });
      return () => void invoke('mic_close');
    },
    // Tabs are a browser thing; system audio comes as a loopback device.
    canCaptureTab: false,
    async liveDevices() {
      return invoke<LiveDevice[]>('live_devices');
    },
    async openLive(source) {
      if (source.kind !== 'device') throw new Error('The desktop app plays along with system audio or an input.');
      await invoke('live_open', { id: source.id }).catch((e: unknown) => {
        throw new Error(String(e));
      });
      return () => void invoke('live_close');
    },
    resetLive() {
      void invoke('live_reset');
    },
    async close() {
      listeners.clear();
      chordListeners.clear();
      songListeners.clear();
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

/**
 * Analyses the last song captured from the live source (see `onLiveSong`),
 * kept in the desktop app, and returns the analysis as JSON.
 */
export async function analyzeLiveSong(onProgress: (fraction: number, step: number) => void, signal?: AbortSignal): Promise<string> {
  const channel = new Channel<{ fraction: number; step: number }>();
  channel.onmessage = (p) => onProgress(p.fraction, p.step);
  const cancel = () => void invoke('audio_cancel');
  signal?.addEventListener('abort', cancel);
  try {
    return await invoke<string>('live_song_analyze', { onProgress: channel });
  } catch (e) {
    throw new Error(String(e));
  } finally {
    signal?.removeEventListener('abort', cancel);
  }
}
