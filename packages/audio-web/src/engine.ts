import { decodeEvent, encodeCommand, type EngineCommand, type TimedEvent } from './protocol';
import { RecordRing } from './ring';
import type { PortMessage } from './worklet';
import workletUrl from './worklet.ts?worker&url';

// Duplicated from worklet.ts: importing it here would run registerProcessor on the main thread.
const PROCESSOR_NAME = 'midzer-engine';
const POLL_MS = 4;

export type Transport = 'shared-memory' | 'message-port' | 'native';

/**
 * Receives each engine event with `timeMs`, the `performance.now()` time at
 * which it reaches the speakers. MIDI out can pass that straight to
 * `MIDIOutput.send` so external gear lines up with the built-in sound.
 */
export type EventListener = (event: TimedEvent, timeMs: number) => void;

export interface MicOptions {
  /** Off with headphones: echo cancellation blurs pitch, but with speakers it keeps the band out. */
  echoCancellation: boolean;
}

/** The engine running in an AudioWorklet (web) or a native audio thread (desktop). */
export interface EngineHandle {
  readonly sampleRate: number;
  /** How long audio takes from the engine to the speakers, in ms. */
  readonly outputLatencyMs: number;
  /** How commands reach the audio thread; shared memory needs cross-origin isolation. */
  readonly transport: Transport;
  send(cmd: EngineCommand): void;
  /** Returns an unsubscribe function. */
  onEvent(listener: EventListener): () => void;
  /** Loads an SF2 file; resolves false if it could not be parsed. */
  loadSoundFont(bytes: ArrayBuffer): Promise<boolean>;
  /**
   * Feeds the microphone to the engine. Resolves a function that stops it;
   * rejects if there is no microphone or access is denied.
   */
  openMic(options: MicOptions): Promise<() => void>;
  close(): Promise<void>;
}

/** Whether the page runs inside the desktop app, where the engine is native (see native.ts). */
export function isNativeHost(): boolean {
  return typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;
}

/** Loads the Rust engine into an AudioWorklet. Call from a user gesture. */
export async function startEngine(wasmUrl: string): Promise<EngineHandle> {
  const context = new AudioContext({ latencyHint: 'interactive' });
  const [module] = await Promise.all([
    WebAssembly.compileStreaming(fetch(wasmUrl)),
    context.audioWorklet.addModule(workletUrl),
  ]);

  const shared = globalThis.crossOriginIsolated;
  // Large enough for a whole song chart (see chartCommands) sent at once.
  const commands = shared ? RecordRing.create(8192) : null;
  const events = shared ? RecordRing.create(4096) : null;
  const node = new AudioWorkletNode(context, PROCESSOR_NAME, {
    numberOfInputs: 1,
    channelCount: 1,
    channelCountMode: 'explicit',
    outputChannelCount: [2],
    processorOptions: { module, commands: commands?.buffer ?? null, events: events?.buffer ?? null },
  });
  node.connect(context.destination);
  await context.resume();

  const listeners = new Set<EventListener>();
  const sampleRate = context.sampleRate;

  /** Maps an AudioContext frame to the performance.now() time it is heard. */
  const timeOfFrame = (frame: number) => {
    const { contextTime = context.currentTime, performanceTime = performance.now() } = context.getOutputTimestamp();
    // Frames travel as u32; unwrap against the current position.
    const now = contextTime * sampleRate;
    const unwrapped = frame + Math.round((now - frame) / 2 ** 32) * 2 ** 32;
    return performanceTime + ((unwrapped - now) / sampleRate) * 1000;
  };

  const dispatch = (op: number, a: number, b: number, frame: number) => {
    const event = decodeEvent(op, a, b, frame);
    if (!event) return;
    const timeMs = timeOfFrame(frame);
    for (const listener of listeners) listener(event, timeMs);
  };

  let pendingSoundFont: ((ok: boolean) => void) | null = null;
  node.port.onmessage = (e: MessageEvent<PortMessage>) => {
    const msg = e.data;
    if (msg.type === 'events') {
      const w = msg.words;
      for (let i = 0; i < w.length; i += 4) dispatch(w[i]!, w[i + 1]!, w[i + 2]!, w[i + 3]!);
    } else if (msg.type === 'soundfontLoaded') {
      pendingSoundFont?.(msg.ok);
      pendingSoundFont = null;
    } else if (msg.type === 'overflow') {
      console.warn(`midzer: dropped ${msg.dropped} engine events (UI too slow)`);
    }
  };

  const record = new Uint32Array(4);
  const poll = events
    ? setInterval(() => {
        while (events.pop(record)) dispatch(record[0]!, record[1]!, record[2]!, record[3]!);
      }, POLL_MS)
    : undefined;

  const { outputLatency = 0, baseLatency = 0 } = context;
  return {
    sampleRate,
    outputLatencyMs: Math.round((outputLatency || baseLatency) * 1000),
    transport: shared ? 'shared-memory' : 'message-port',
    send(cmd) {
      const words = encodeCommand(cmd);
      if (commands) {
        if (!commands.push(words)) console.warn('midzer: command ring full, dropped', cmd);
      } else {
        node.port.postMessage({ type: 'command', words } satisfies PortMessage);
      }
    },
    onEvent(listener) {
      listeners.add(listener);
      return () => listeners.delete(listener);
    },
    loadSoundFont(bytes) {
      return new Promise((resolve) => {
        pendingSoundFont?.(false);
        pendingSoundFont = resolve;
        node.port.postMessage({ type: 'soundfont', bytes } satisfies PortMessage, [bytes]);
      });
    },
    async openMic({ echoCancellation }) {
      const stream = await navigator.mediaDevices.getUserMedia({
        audio: { echoCancellation, noiseSuppression: false, autoGainControl: false, channelCount: 1 },
      });
      const source = new MediaStreamAudioSourceNode(context, { mediaStream: stream });
      source.connect(node);
      return () => {
        source.disconnect();
        for (const track of stream.getTracks()) track.stop();
      };
    },
    async close() {
      clearInterval(poll);
      listeners.clear();
      node.disconnect();
      await context.close();
    },
  };
}
