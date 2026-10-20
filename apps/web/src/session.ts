// App state and the wiring between UI, engine and MIDI devices.

import {
  BASS_CHANNEL,
  DRUM_CHANNEL,
  eventToMidi,
  isMidiSupported,
  KEYS_CHANNEL,
  listenToInputs,
  PAD_CHANNEL,
  requestMidi,
  startEngine,
  STYLES,
  type BandPart,
  type BandState,
  type ChordInfo,
  type EngineCommand,
  type EngineHandle,
  type IdleMode,
  type SectionRequest,
  type TimedEvent,
} from '@midzer/audio-web';
import { create } from 'zustand';
import { enqueue, flushLive, resetLive } from './live';
import { qwertyToNote } from './qwerty';

export type Screen = 'onboarding' | 'home' | 'jam';
export type AudioStatus = 'idle' | 'starting' | 'loading' | 'ready' | 'error';

export const BAND_CHANNELS: Record<BandPart, number> = {
  drums: DRUM_CHANNEL,
  bass: BASS_CHANNEL,
  keys: KEYS_CHANNEL,
  pad: PAD_CHANNEL,
};

export const SPLIT_OPTIONS = [
  { label: 'Whole keyboard', note: 0 },
  { label: 'Split at C4', note: 60 },
  { label: 'Split at F#3', note: 54 },
] as const;

export interface MidiPort {
  id: string;
  name: string;
}

interface SessionState {
  screen: Screen;
  onboardingStep: 0 | 1 | 2;
  audio: AudioStatus;
  /** SoundFont download progress, 0..1. */
  loadProgress: number;
  error: string | null;
  midiSupported: boolean;
  midiInputs: MidiPort[];
  midiOutputs: MidiPort[];
  midiOutputId: string | null;
  playing: boolean;
  bpm: number;
  chord: ChordInfo | null;
  bar: number;
  beat: number;
  muted: Record<BandPart, boolean>;
  split: number;
  volume: number;
  /** The style the player picked; `band.style` is the one being heard. */
  style: number;
  band: BandState;
  idleMode: IdleMode;
  /** How far the player's taps land after the heard beat, from calibration. */
  tapOffsetMs: number | null;
  /** Output latency of the audio device, once audio has started. */
  outputLatencyMs: number;
}

export const IDLE_OPTIONS: { mode: IdleMode; label: string }[] = [
  { mode: 'rest', label: 'Band waits for you' },
  { mode: 'end', label: 'Band plays the ending' },
  { mode: 'keepPlaying', label: 'Band keeps playing' },
];

const ONBOARDED_KEY = 'midzer.onboarded';
const PREFS_KEY = 'midzer.prefs';

function readOnboarded(): boolean {
  try {
    return localStorage.getItem(ONBOARDED_KEY) === '1';
  } catch {
    return false;
  }
}

interface Prefs {
  style: number;
  idleMode: IdleMode;
  tapOffsetMs: number | null;
}

function readPrefs(): Prefs {
  const defaults: Prefs = { style: 0, idleMode: 'rest', tapOffsetMs: null };
  try {
    const saved = JSON.parse(localStorage.getItem(PREFS_KEY) ?? '{}') as Partial<Prefs>;
    const style = Number.isInteger(saved.style) && STYLES[saved.style!] ? saved.style! : defaults.style;
    const idleMode = IDLE_OPTIONS.some((o) => o.mode === saved.idleMode) ? saved.idleMode! : defaults.idleMode;
    const tapOffsetMs = typeof saved.tapOffsetMs === 'number' ? saved.tapOffsetMs : null;
    return { style, idleMode, tapOffsetMs };
  } catch {
    return defaults;
  }
}

function savePrefs(): void {
  const { style, idleMode, tapOffsetMs } = get();
  try {
    localStorage.setItem(PREFS_KEY, JSON.stringify({ style, idleMode, tapOffsetMs } satisfies Prefs));
  } catch {
    // Not persisted; defaults next time.
  }
}

const prefs = readPrefs();

export const useSession = create<SessionState>(() => ({
  screen: readOnboarded() ? 'home' : 'onboarding',
  onboardingStep: 0,
  audio: 'idle',
  loadProgress: 0,
  error: null,
  midiSupported: isMidiSupported(),
  midiInputs: [],
  midiOutputs: [],
  midiOutputId: null,
  playing: false,
  bpm: STYLES[prefs.style]!.bpm,
  chord: null,
  bar: 0,
  beat: 0,
  muted: { drums: false, bass: false, keys: false, pad: false },
  split: 0,
  volume: 0.6,
  style: prefs.style,
  band: {
    style: prefs.style,
    nextStyle: null,
    playing: false,
    section: 'main',
    variation: 0,
    nextVariation: 0,
    fillPending: false,
    introPending: false,
    endingPending: false,
    follow: true,
    energy: 0.4,
  },
  idleMode: prefs.idleMode,
  tapOffsetMs: prefs.tapOffsetMs,
  outputLatencyMs: 0,
}));

const set = useSession.setState;
const get = useSession.getState;

let engine: EngineHandle | null = null;
let midiAccess: MIDIAccess | null = null;

// ---- Navigation -------------------------------------------------------------

export function goTo(screen: Screen): void {
  set({ screen });
}

export function setOnboardingStep(step: 0 | 1 | 2): void {
  set({ onboardingStep: step });
}

export function finishOnboarding(): void {
  try {
    localStorage.setItem(ONBOARDED_KEY, '1');
  } catch {
    // Private mode etc.: onboarding just shows again next time.
  }
  set({ screen: 'jam' });
}

// ---- Engine -----------------------------------------------------------------

export function send(cmd: EngineCommand): void {
  engine?.send(cmd);
}

/** Starts audio, sounds and MIDI. Must be called from a user gesture. */
export async function startAudio(): Promise<void> {
  if (get().audio !== 'idle' && get().audio !== 'error') return;
  set({ audio: 'starting', error: null });
  try {
    engine = await startEngine('/wasm/midzer.wasm');
    engine.onEvent(onEngineEvent);
    const { bpm, volume, split, muted, style, idleMode } = get();
    const { outputLatency = 0, baseLatency = 0 } = engine.context;
    set({ outputLatencyMs: Math.round((outputLatency || baseLatency) * 1000) });
    engine.send({ type: 'setStyle', index: style });
    engine.send({ type: 'setIdleMode', mode: idleMode });
    sendLatency();
    engine.send({ type: 'setTempo', bpm });
    engine.send({ type: 'setGain', gain: volume });
    engine.send({ type: 'setSplit', note: split });
    for (const [part, isMuted] of Object.entries(muted)) {
      if (isMuted) engine.send({ type: 'setMute', channel: BAND_CHANNELS[part as BandPart], muted: true });
    }
    requestAnimationFrame(frameLoop);
    void connectMidi();

    set({ audio: 'loading' });
    const bytes = await download('/soundfonts/GeneralUser-GS.sf2', (p) => set({ loadProgress: p }));
    if (!(await engine.loadSoundFont(bytes))) throw new Error('The instrument sounds could not be loaded.');
    set({ audio: 'ready', loadProgress: 1 });
  } catch (e) {
    set({ audio: 'error', error: e instanceof Error ? e.message : String(e) });
  }
}

function frameLoop(): void {
  flushLive();
  requestAnimationFrame(frameLoop);
}

function onEngineEvent(event: TimedEvent, timeMs: number): void {
  enqueue(event, timeMs);
  sendMidiOut(event, timeMs);

  // UI state changes when the change is heard, not when it is scheduled.
  const delay = Math.max(0, timeMs - performance.now());
  switch (event.type) {
    case 'chord':
      setTimeout(() => set({ chord: event.chord }), delay);
      break;
    case 'beat':
      setTimeout(() => set({ bar: event.bar, beat: event.beat }), delay);
      break;
    case 'transport':
      setTimeout(() => set({ playing: event.playing }), delay);
      break;
    case 'band':
      setTimeout(() => set({ band: event.state }), delay);
      break;
  }
  for (const listener of beatListeners) if (event.type === 'beat') listener(event.bar, event.beat, timeMs);
}

type BeatListener = (bar: number, beat: number, timeMs: number) => void;
const beatListeners = new Set<BeatListener>();

/** Calls `listener` with the time each beat is heard. Returns an unsubscribe function. */
export function onBeat(listener: BeatListener): () => void {
  beatListeners.add(listener);
  return () => beatListeners.delete(listener);
}

async function download(url: string, onProgress: (fraction: number) => void): Promise<ArrayBuffer> {
  const res = await fetch(url);
  if (!res.ok || !res.body) throw new Error(`Could not download sounds (HTTP ${res.status}).`);
  const total = Number(res.headers.get('content-length')) || 0;
  const reader = res.body.getReader();
  const chunks: Uint8Array[] = [];
  let received = 0;
  for (;;) {
    const { done, value } = await reader.read();
    if (done) break;
    chunks.push(value);
    received += value.length;
    if (total) onProgress(received / total);
  }
  const bytes = new Uint8Array(received);
  let offset = 0;
  for (const chunk of chunks) {
    bytes.set(chunk, offset);
    offset += chunk.length;
  }
  return bytes.buffer;
}

// ---- Transport & band -------------------------------------------------------

export function togglePlay(): void {
  send({ type: get().playing ? 'stop' : 'play' });
}

export function setTempo(bpm: number): void {
  const clamped = Math.round(Math.min(240, Math.max(40, bpm)));
  set({ bpm: clamped });
  send({ type: 'setTempo', bpm: clamped });
}

export function toggleMute(part: BandPart): void {
  const muted = { ...get().muted, [part]: !get().muted[part] };
  set({ muted });
  send({ type: 'setMute', channel: BAND_CHANNELS[part], muted: muted[part] });
}

export function setSplit(note: number): void {
  set({ split: note });
  send({ type: 'setSplit', note });
}

export function setVolume(volume: number): void {
  set({ volume });
  send({ type: 'setGain', gain: volume });
}

/** Picks a style. Stopped, it also takes the style's tempo; playing, it changes on the next bar. */
export function setStyle(index: number): void {
  const style = STYLES[index];
  if (!style) return;
  set({ style: index });
  savePrefs();
  send({ type: 'setStyle', index });
  if (!get().playing) setTempo(style.bpm);
}

/** Intro, A/B/C, fill or ending. Choosing a variation stops following the player. */
export function requestSection(request: SectionRequest): void {
  send({ type: 'section', request });
}

export function setFollow(on: boolean): void {
  send({ type: 'setFollow', on });
}

/** Sets the energy by hand, which stops following the player. */
export function setEnergy(energy: number): void {
  send({ type: 'setFollow', on: false });
  send({ type: 'setEnergy', energy });
  // Show the new value at once instead of after the audio latency.
  set({ band: { ...get().band, follow: false, energy } });
}

export function setIdleMode(mode: IdleMode): void {
  set({ idleMode: mode });
  savePrefs();
  send({ type: 'setIdleMode', mode });
}

/** How late, in ms, the player's notes reach the engine after the beat they meant. */
export function playerLatencyMs(): number {
  const { outputLatencyMs, tapOffsetMs } = get();
  return Math.max(0, outputLatencyMs + (tapOffsetMs ?? 0));
}

function sendLatency(): void {
  send({ type: 'setLatency', ms: playerLatencyMs() });
}

export function setTapOffset(ms: number | null): void {
  set({ tapOffsetMs: ms === null ? null : Math.round(ms) });
  savePrefs();
  sendLatency();
}

/** Calibration: the band plays only a click on every beat while `on`. */
export function setCalibrationClick(on: boolean): void {
  send({ type: 'stop' });
  send({ type: 'setClick', on });
  if (on) send({ type: 'play' });
}

const taps: number[] = [];

/** Tap tempo: averages the last few taps; a pause of 2 s starts over. */
export function tapTempo(now = performance.now()): void {
  if (taps.length > 0 && now - taps[taps.length - 1]! > 2000) taps.length = 0;
  taps.push(now);
  if (taps.length > 5) taps.shift();
  if (taps.length >= 2) {
    const avg = (taps[taps.length - 1]! - taps[0]!) / (taps.length - 1);
    setTempo(60_000 / avg);
  }
}

// ---- Player input -----------------------------------------------------------

export function playerNoteOn(note: number, velocity = 100): void {
  send({ type: 'noteOn', note, velocity });
}

export function playerNoteOff(note: number): void {
  send({ type: 'noteOff', note });
}

function isTyping(target: EventTarget | null): boolean {
  return target instanceof HTMLElement && (target.isContentEditable || /^(INPUT|SELECT|TEXTAREA)$/.test(target.tagName));
}

/** Computer-keyboard playing. Returns a cleanup function. */
export function attachQwerty(): () => void {
  const down = (e: KeyboardEvent) => {
    if (isTyping(e.target) || e.metaKey || e.ctrlKey || e.altKey) return;
    if (e.code === 'Space') {
      e.preventDefault();
      if (!e.repeat) togglePlay();
      return;
    }
    const note = qwertyToNote(e.key);
    if (note !== undefined && !e.repeat) playerNoteOn(note);
  };
  const up = (e: KeyboardEvent) => {
    const note = qwertyToNote(e.key);
    if (note !== undefined) playerNoteOff(note);
  };
  const blur = () => {
    send({ type: 'allNotesOff' });
    resetLive();
  };
  window.addEventListener('keydown', down);
  window.addEventListener('keyup', up);
  window.addEventListener('blur', blur);
  return () => {
    window.removeEventListener('keydown', down);
    window.removeEventListener('keyup', up);
    window.removeEventListener('blur', blur);
  };
}

// ---- MIDI devices -----------------------------------------------------------

async function connectMidi(): Promise<void> {
  midiAccess = await requestMidi();
  if (!midiAccess) return;
  listenToInputs(midiAccess, send);
  const refresh = () => {
    const ports = (map: MIDIInputMap | MIDIOutputMap) =>
      [...map.values()].map((p) => ({ id: p.id, name: p.name ?? 'MIDI device' }));
    const outputs = ports(midiAccess!.outputs);
    const { midiOutputId } = get();
    set({
      midiInputs: ports(midiAccess!.inputs),
      midiOutputs: outputs,
      midiOutputId: outputs.some((o) => o.id === midiOutputId) ? midiOutputId : null,
    });
  };
  refresh();
  midiAccess.addEventListener('statechange', refresh);
}

export function setMidiOutput(id: string | null): void {
  silenceMidiOut();
  set({ midiOutputId: id });
}

function currentOutput(): MIDIOutput | undefined {
  const id = get().midiOutputId;
  return id ? midiAccess?.outputs.get(id) : undefined;
}

function sendMidiOut(event: TimedEvent, timeMs: number): void {
  const out = currentOutput();
  if (!out) return;
  const bytes = eventToMidi(event);
  if (bytes) out.send(bytes, timeMs);
  if (event.type === 'transport' && !event.playing) silenceMidiOut(timeMs);
}

function silenceMidiOut(timeMs?: number): void {
  const out = currentOutput();
  if (!out) return;
  for (const channel of Object.values(BAND_CHANNELS)) out.send([0xb0 | channel, 123, 0], timeMs);
}
