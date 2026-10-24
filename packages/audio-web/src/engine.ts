import type { HearReply, HearRequest } from './hear.worker';
import hearWorkerUrl from './hear.worker.ts?worker&url';
import { chordName, decodeEvent, encodeCommand, type ChordInfo, type EngineCommand, type TimedEvent } from './protocol';
import { RecordRing } from './ring';
import { TapRing } from './tap';
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

/** A chord heard in the live source, from when to when (in the source's own time, as `performance.now()`). */
export interface LiveChordSpan {
  fromMs: number;
  toMs: number;
  chord: ChordInfo | null;
  /** Not heard yet: what the song's loop says comes next. */
  predicted: boolean;
}

/**
 * Receives what the listener (crates/midzer-hear) heard of the live source:
 * the chords of the last 10 s, oldest first, replacing what came before
 * about that stretch. The source's time is when it reached midzer; the
 * player may hear it a little later (see `setLiveOffset`).
 */
export type LiveChordListener = (spans: LiveChordSpan[]) => void;

/**
 * Chord spans as the listener sends them: [from, to, code, predicted] in
 * engine frames, code root · 16 + quality or −1, predicted 1 or 0.
 */
export function decodeChordSpans(spans: ArrayLike<number>, timeOfFrame: (frame: number) => number): LiveChordSpan[] {
  const out: LiveChordSpan[] = [];
  for (let i = 0; i + 3 < spans.length; i += 4) {
    const code = spans[i + 2]!;
    const root = Math.floor(code / 16);
    const quality = code % 16;
    const chord = code < 0 ? null : { root, bass: root, quality, name: chordName(root, quality, root) };
    out.push({ fromMs: timeOfFrame(spans[i]!), toMs: timeOfFrame(spans[i + 1]!), chord, predicted: spans[i + 3] === 1 });
  }
  return out;
}

/**
 * A song heard in the live source, from a silence to a silence (or part of
 * one), for the app to analyse and keep. Live sources are captured while
 * open; a song ends at a silence, with `finishLiveSong`, or when the source closes.
 */
export interface LiveSongCapture {
  seconds: number;
  /** Heard from its start to its end; otherwise only part of the song. */
  complete: boolean;
  /** Mono 16-bit samples at 22.05 kHz, in the browser; the desktop app keeps them natively (see `analyzeLiveSong`). */
  samples?: Int16Array;
  /** The stored song (its `LiveLibrarySong` id) followed for long in it: it is that song, heard again. */
  followed?: number;
}

export type LiveSongListener = (song: LiveSongCapture) => void;

/** A stored song for the listener to recognise in live sources and follow (see crates/midzer-hear recognise.rs). */
export interface LiveLibrarySong {
  /** The app's number for it: a whole number, unique in the session. */
  id: number;
  /** Its fingerprint: landmark hash, frame, hash, frame, … */
  landmarks: Uint32Array;
  /** When each chart beat falls, in seconds into the audio the fingerprint came from, plus the end of the last bar. */
  beatTimes: Float64Array;
  /** The chord of each beat: root · 16 + quality, or −1 for none. */
  chords: Int32Array;
  /** How long that audio was. */
  seconds: number;
  /** How many beats each bar of the chart has (empty: all four). */
  barBeats: Uint8Array;
}

/** A stored song recognised in the live source: `seconds` into it at `timeMs` (the source's time, as for chords). */
export interface LiveFollow {
  id: number;
  seconds: number;
  timeMs: number;
}

/** Gets the song followed, or null when none is any more. */
export type LiveFollowListener = (follow: LiveFollow | null) => void;

/** The rate captured songs are in. */
export const LIVE_SONG_RATE = 22_050;

/** A captured song as the listener sends it. */
export function liveSongOf(samples: Int16Array | null, seconds: number, flags: number, followed = -1): LiveSongCapture {
  const song: LiveSongCapture = { seconds, complete: (flags & 3) === 3 };
  if (samples) song.samples = samples;
  if (followed >= 0) song.followed = followed;
  return song;
}

/** A follow as the listener sends it: id −1 for none. */
export function liveFollowOf(id: number, seconds: number, timeMs: number): LiveFollow | null {
  return id < 0 ? null : { id, seconds, timeMs };
}

export interface MicOptions {
  /** Off with headphones: echo cancellation blurs pitch, but with speakers it keeps the band out. */
  echoCancellation: boolean;
}

/** A recording for songs to play instead of the band (the `setTrack` command). */
export type TrackSource =
  /** Already decoded at the engine's sample rate, as interleaved stereo (in the browser). */
  | { kind: 'samples'; main: Int16Array; drums: Int16Array | null }
  /** The file itself, or a song's saved stems with the drums apart, decoded natively (desktop). */
  | { kind: 'file'; bytes: Uint8Array; stemsDir: string | null };

/** Live audio to play along with (the `setLive` command). */
export type LiveSource =
  /** A browser tab's audio, picked in the browser's share dialog (Chrome and Edge). */
  | { kind: 'tab' }
  /** An audio input from `liveDevices`: a line input, or system audio where the platform offers it. */
  | { kind: 'device'; id: string };

export interface LiveDevice {
  id: string;
  name: string;
  /** It records what the computer plays (a monitor or loopback), not a microphone or line in. */
  loopback: boolean;
}

export interface TrackLoaded {
  seconds: number;
  /** Whether the drums are a separate stem that `setTrackDrums` can mute. */
  drumStem: boolean;
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
  /** Chords the listener hears in the live source; returns an unsubscribe function. Needs cross-origin isolation in the browser. */
  onLiveChords(listener: LiveChordListener): () => void;
  /** Songs heard in live sources; returns an unsubscribe function. */
  onLiveSong(listener: LiveSongListener): () => void;
  /** Whether live songs are captured at all (the listener runs; in the browser it needs cross-origin isolation). */
  readonly canLearnSongs: boolean;
  /** Ends the live song being captured here, so `onLiveSong` gets it now. */
  finishLiveSong(): void;
  /** The stored songs to recognise in live sources, replacing those before. */
  setLiveLibrary(songs: LiveLibrarySong[]): void;
  /** The stored song recognised and followed; returns an unsubscribe function. */
  onLiveFollow(listener: LiveFollowListener): () => void;
  /** "Not this song": stops following it until the next silence or `resetLive`. */
  ignoreLiveSong(id: number): void;
  /** Loads an SF2 file; resolves false if it could not be parsed. */
  loadSoundFont(bytes: ArrayBuffer): Promise<boolean>;
  /** Hands the engine a backing track (its sample arrays are moved), or frees it with null. Rejects if it can't be read. */
  loadTrack(source: TrackSource | null): Promise<TrackLoaded | null>;
  /**
   * Feeds the microphone to the engine. Resolves a function that stops it;
   * rejects if there is no microphone or access is denied.
   */
  openMic(options: MicOptions): Promise<() => void>;
  /** Whether `openLive` can capture a browser tab. */
  readonly canCaptureTab: boolean;
  /** Inputs live audio can come from; names may be missing until audio input is allowed. */
  liveDevices(): Promise<LiveDevice[]>;
  /**
   * Feeds live audio to the engine's beat tracker. Resolves a function that
   * stops it; rejects if it can't be opened. `onEnded` runs if the source
   * stops by itself (e.g. the browser's "Stop sharing").
   */
  openLive(source: LiveSource, onEnded: () => void): Promise<() => void>;
  /** Forgets the live song, in the engine and the listener, and listens afresh (`liveReset`). */
  resetLive(): void;
  close(): Promise<void>;
}

/** Input names that are the computer's own output rather than a microphone. */
const LOOPBACK_NAME = /monitor|loopback|stereo mix|what u hear|blackhole|soundflower/i;

/** Whether the page runs inside the desktop app, where the engine is native (see native.ts). */
export function isNativeHost(): boolean {
  return typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;
}

/**
 * Loads the Rust engine into an AudioWorklet. Call from a user gesture.
 * `hearUrl` is the listener (hear.wasm) that follows live sources with its
 * networks; without it, or without cross-origin isolation, the engine's own
 * beat tracker follows them alone.
 */
export async function startEngine(wasmUrl: string, hearUrl?: string): Promise<EngineHandle> {
  const context = new AudioContext({ latencyHint: 'interactive' });
  const [module] = await Promise.all([
    WebAssembly.compileStreaming(fetch(wasmUrl)),
    context.audioWorklet.addModule(workletUrl),
  ]);

  const shared = globalThis.crossOriginIsolated;
  // Large enough for a whole song chart (see chartCommands) sent at once.
  const commands = shared ? RecordRing.create(8192) : null;
  const events = shared ? RecordRing.create(4096) : null;
  const tap = shared && hearUrl ? TapRing.create() : null;
  // Input 0 is the microphone, input 1 the live source.
  const node = new AudioWorkletNode(context, PROCESSOR_NAME, {
    numberOfInputs: 2,
    channelCount: 1,
    channelCountMode: 'explicit',
    outputChannelCount: [2],
    processorOptions: { module, commands: commands?.buffer ?? null, events: events?.buffer ?? null, tap: tap?.buffer ?? null },
  });
  node.connect(context.destination);
  await context.resume();

  const listeners = new Set<EventListener>();
  const chordListeners = new Set<LiveChordListener>();
  const songListeners = new Set<LiveSongListener>();
  const followListeners = new Set<LiveFollowListener>();
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
  let pendingTrack: ((frames: number) => void) | null = null;
  node.port.onmessage = (e: MessageEvent<PortMessage>) => {
    const msg = e.data;
    if (msg.type === 'events') {
      const w = msg.words;
      for (let i = 0; i < w.length; i += 4) dispatch(w[i]!, w[i + 1]!, w[i + 2]!, w[i + 3]!);
    } else if (msg.type === 'soundfontLoaded') {
      pendingSoundFont?.(msg.ok);
      pendingSoundFont = null;
    } else if (msg.type === 'trackLoaded') {
      pendingTrack?.(msg.frames);
      pendingTrack = null;
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

  const send = (cmd: EngineCommand) => {
    const words = encodeCommand(cmd);
    if (commands) {
      if (!commands.push(words)) console.warn('midzer: command ring full, dropped', cmd);
    } else {
      node.port.postMessage({ type: 'command', words } satisfies PortMessage);
    }
  };

  // The listener starts with the first live source and stays for the session.
  let hearing: Worker | null = null;
  let library: LiveLibrarySong[] | null = null;
  const startHearing = () => {
    if (!tap || !hearUrl || hearing) return;
    hearing = new Worker(hearWorkerUrl, { type: 'module' });
    hearing.onmessage = (e: MessageEvent<HearReply>) => {
      const msg = e.data;
      if (msg.type === 'grid') {
        send({ type: 'liveGuide', frame: msg.frame, period: msg.period, one: msg.one, confidence: msg.confidence });
      } else if (msg.type === 'chords') {
        const offset = tap.frameOffset;
        const spans = decodeChordSpans(msg.spans, (frame) => timeOfFrame(frame + offset));
        for (const listener of chordListeners) listener(spans);
      } else if (msg.type === 'song') {
        const song = liveSongOf(msg.samples, msg.samples.length / LIVE_SONG_RATE, msg.flags, msg.followed);
        for (const listener of songListeners) listener(song);
      } else if (msg.type === 'bars') {
        for (let i = 0; i + 1 < msg.bars.length; i += 2) send({ type: 'liveBar', frame: msg.bars[i]!, beats: msg.bars[i + 1]! });
      } else if (msg.type === 'follow') {
        const follow = liveFollowOf(msg.id, msg.seconds, timeOfFrame(msg.frame + tap.frameOffset));
        for (const listener of followListeners) listener(follow);
      } else {
        console.warn(`midzer: the listener stopped: ${msg.message}`);
        tap.enabled = false;
      }
    };
    hearing.postMessage({ type: 'start', wasmUrl: hearUrl, tap: tap.buffer, sampleRate } satisfies HearRequest);
    hearing.postMessage({ type: 'capture', on: true } satisfies HearRequest);
    if (library) hearing.postMessage({ type: 'library', songs: library } satisfies HearRequest);
  };

  const { outputLatency = 0, baseLatency = 0 } = context;
  return {
    sampleRate,
    outputLatencyMs: Math.round((outputLatency || baseLatency) * 1000),
    transport: shared ? 'shared-memory' : 'message-port',
    send,
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
    canLearnSongs: tap !== null,
    finishLiveSong() {
      hearing?.postMessage({ type: 'finish' } satisfies HearRequest);
    },
    setLiveLibrary(songs) {
      library = songs;
      hearing?.postMessage({ type: 'library', songs } satisfies HearRequest);
    },
    onLiveFollow(listener) {
      followListeners.add(listener);
      return () => followListeners.delete(listener);
    },
    ignoreLiveSong(id) {
      hearing?.postMessage({ type: 'ignore', id } satisfies HearRequest);
    },
    loadSoundFont(bytes) {
      return new Promise((resolve) => {
        pendingSoundFont?.(false);
        pendingSoundFont = resolve;
        node.port.postMessage({ type: 'soundfont', bytes } satisfies PortMessage, [bytes]);
      });
    },
    async loadTrack(source) {
      if (source && source.kind !== 'samples') throw new Error('The browser engine takes decoded samples.');
      // Moved, not copied: the arrays are unusable afterwards.
      const own = (a: Int16Array | null | undefined) =>
        !a ? null : a.byteOffset === 0 && a.byteLength === a.buffer.byteLength ? (a.buffer as ArrayBuffer) : a.slice().buffer;
      const main = own(source?.main);
      const drums = own(source?.drums);
      const frames = await new Promise<number>((resolve) => {
        pendingTrack?.(0);
        pendingTrack = resolve;
        const transfer = [main, drums].filter((b): b is ArrayBuffer => b !== null);
        node.port.postMessage({ type: 'track', main, drums } satisfies PortMessage, transfer);
      });
      return source ? { seconds: frames / sampleRate, drumStem: source.drums !== null } : null;
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
    canCaptureTab: typeof navigator !== 'undefined' && typeof navigator.mediaDevices?.getDisplayMedia === 'function',
    async liveDevices() {
      const devices = await navigator.mediaDevices.enumerateDevices();
      return devices
        .filter((d) => d.kind === 'audioinput')
        .map((d, i) => ({ id: d.deviceId, name: d.label || `Audio input ${i + 1}`, loopback: LOOPBACK_NAME.test(d.label) }))
        .sort((a, b) => Number(b.loopback) - Number(a.loopback));
    },
    async openLive(source, onEnded) {
      const raw = { echoCancellation: false, noiseSuppression: false, autoGainControl: false };
      let stream: MediaStream;
      if (source.kind === 'tab') {
        // Chrome only shares audio along with a picture; the picture is dropped at once.
        stream = await navigator.mediaDevices.getDisplayMedia({
          video: true,
          audio: { ...raw, suppressLocalAudioPlayback: false },
          systemAudio: 'include',
          selfBrowserSurface: 'exclude',
          preferCurrentTab: false,
        } as DisplayMediaStreamOptions);
        for (const track of stream.getVideoTracks()) track.stop();
        if (stream.getAudioTracks().length === 0) {
          throw new Error('No sound was shared. Pick a tab and turn on sharing its audio.');
        }
      } else {
        stream = await navigator.mediaDevices.getUserMedia({ audio: { ...raw, deviceId: { exact: source.id } } });
      }
      const input = new MediaStreamAudioSourceNode(context, { mediaStream: stream });
      input.connect(node, 0, 1);
      startHearing();
      hearing?.postMessage({ type: 'reset' } satisfies HearRequest);
      if (tap) tap.enabled = true;
      let open = true;
      const stop = () => {
        if (!open) return;
        open = false;
        if (tap) tap.enabled = false;
        // The song so far is offered before the listener forgets it.
        hearing?.postMessage({ type: 'finish' } satisfies HearRequest);
        hearing?.postMessage({ type: 'reset' } satisfies HearRequest);
        input.disconnect();
        for (const track of stream.getTracks()) track.stop();
      };
      stream.getAudioTracks()[0]?.addEventListener('ended', () => {
        if (!open) return;
        stop();
        onEnded();
      });
      return stop;
    },
    resetLive() {
      hearing?.postMessage({ type: 'reset' } satisfies HearRequest);
      send({ type: 'liveReset' });
    },
    async close() {
      clearInterval(poll);
      hearing?.terminate();
      listeners.clear();
      chordListeners.clear();
      songListeners.clear();
      followListeners.clear();
      node.disconnect();
      await context.close();
    },
  };
}
