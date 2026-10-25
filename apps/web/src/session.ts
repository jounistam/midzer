// App state and the wiring between UI, engine and MIDI devices.

import {
  BASS_CHANNEL,
  DRUM_CHANNEL,
  eventToMidi,
  isMidiSupported,
  isNativeHost,
  KEYS_CHANNEL,
  LISTEN_DELAY_MS,
  listenToInputs,
  MIC_CHANNEL,
  PAD_CHANNEL,
  requestMidi,
  startEngine,
  STYLES,
  type BandDrums,
  type BandPart,
  type BandState,
  type ChordInfo,
  type DrumPiece,
  type EngineCommand,
  type EngineHandle,
  type IdleMode,
  type Instrument,
  type KitAction,
  type KitTrigger,
  type ListenMode,
  type LiveChordSpan,
  type LiveDevice,
  type LiveFollow,
  type LiveLibrarySong,
  type LiveSongCapture,
  type LiveSource,
  type NativeMidi,
  type SectionRequest,
  type TimedEvent,
  type TrackSource,
} from '@midzer/audio-web';
import { create } from 'zustand';
import { enqueue, flushLive, live, resetLive } from './live';
import {
  assignTrigger,
  kitCommands,
  kitMap,
  parseKitSetup,
  parseKitTriggers,
  qwertyToPiece,
  triggerCommands,
  type KitSetup,
  type KitTriggers,
} from './drums/kit';
import { pickProgression, progressionCommands } from './drums/progressions';
import { ChordTimeline, keyHints, snapToBeats, type ChordNow, type HeardChord } from './playalong/chords';
import { chordBits, KeyFollower, keyOf, pentatonic, type SongKey, type WeightedChord } from './playalong/scale';
import { homeKey, parseAlongMode, parseSoloLevel, soloHints, type AlongMode, type SoloLevel } from './playalong/solo';
import { parseLevel, type AlongLevel } from './playalong/levels';
import { MISS_SOUND_OPTIONS, missOf, playMiss, type MissSound } from './playalong/feedback';
import type { JudgedHit } from './drums/judging';
import { COUNT_IN_OPTIONS, countInPieces, type CountInChoice } from './drums/stage';
import { offsetToBeat, SYNC_TAPS, syncedOffset } from './playalong/sync';
import { qwertyToNote } from './qwerty';
import { parseSound } from './sounds';
import { isBuiltin, loadUserSongs, saveUserSong } from './songs/library';
import { captureFate, LiveIds, liveLibrarySong } from './songs/liveLibrary';
import { analyzeAudio, analyzeCapture, decodeRecording } from './songs/analyzeAudio';
import { loadFingerprint, saveFingerprint } from './songs/fingerprints';
import { songFromAnalysis, transcriptionMidi } from './songs/audio';
import { Recorder } from './songs/recorder';
import { loadRecording } from './songs/recordings';
import { melodyCommands } from './songs/melody';
import { barAt, barStarts, chartCommands, chordsUsed, newSongId, resolvedBeats, trackCommands, type Bar, type Song } from './songs/song';
import { parseChord } from './songs/chords';
import { keepFixes, timelineOf, withTimeline } from './songs/timeline';
import { recordingEnvelope } from './songs/envelope';

export type Screen = 'onboarding' | 'home' | 'jam' | 'songs' | 'song' | 'learn' | 'lesson' | 'along';

/** Screens that play an open song, so leaving them closes it. */
const SONG_SCREENS: Screen[] = ['song', 'lesson'];

/** Bars `start..end` of the open song repeat. */
export interface LoopRange {
  start: number;
  end: number;
}
export type AudioStatus = 'idle' | 'starting' | 'loading' | 'ready' | 'error';

/** The open song's recording: being read, ready to play along with, or why not. */
export type RecordingState =
  | { status: 'none' }
  | { status: 'loading' }
  | { status: 'ready'; seconds: number; drumStem: boolean }
  | { status: 'error'; message: string };

/** The live source being played along with. */
export type LiveSourceState =
  | { status: 'none' }
  | { status: 'opening'; key: string }
  | { status: 'on'; key: string }
  /** It stopped by itself, e.g. the browser's "Stop sharing". */
  | { status: 'ended'; key: string }
  | { status: 'error'; key: string; message: string };

/** A song learned from the live source, offered to keep. */
export interface LearnedSong {
  song: Song;
  notes: string[];
  fingerprint: Uint32Array;
  /** More of a song kept only in part (its id and title): keeping it updates that song. */
  extends?: { id: string; title: string };
}

/** A song kept that the listener recognised in the live source and follows. */
export interface LiveFollowed {
  songId: string;
  title: string;
  bpm: number;
  /** Every chord the song uses, in the order they first come. */
  chords: string[];
}

/** Songs learned from the live source: the one being analysed, those offered (newest first), and why the last one failed. */
export interface LiveLearning {
  analysing: { fraction: number; seconds: number } | null;
  offered: LearnedSong[];
  error: string | null;
}

/** What the beat tracker says about the live source. */
export interface LiveTracker {
  locked: boolean;
  /** 0..1 */
  confidence: number;
  /** Loudness, 0..1 over -80..0 dBFS. */
  level: number;
}

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
  /** The song open in the song screen, with any unsaved edits. */
  song: Song | null;
  songDirty: boolean;
  /** How many edits of the open song can be undone and redone. */
  songHistory: { undo: number; redo: number };
  /** What the user should know about an imported song. */
  songNotes: string[];
  /** The song bar being heard; null when stopped. */
  songBar: number | null;
  /** Where the song starts on Play. */
  songStartBar: number;
  loop: LoopRange | null;
  /** A recorded performance is ready to save as MIDI. */
  hasTake: boolean;
  /** What the microphone listens for; it is only open while this isn't 'off'. */
  listenMode: ListenMode;
  /** Sounds quieter than this (dBFS) are ignored. */
  micGateDb: number;
  /** With headphones the mic skips echo cancellation, which hears pitch better. */
  headphones: boolean;
  micError: string | null;
  /** The band speeds up and slows down with the player. */
  tempoFollow: boolean;
  /** Keys or drums: what the player's notes are and what the screen shows. */
  instrument: Instrument;
  /** Which note of the e-kit plays which piece. */
  kit: KitSetup;
  /** The built-in synth sounds the player's drums; off when the kit's own module does. */
  kitSound: boolean;
  /** Which pieces count the band in for a drummer. */
  countIn: CountInChoice;
  /** Even count-in hits heard so far, 0–4. */
  countInHits: number;
  /** What the band's own drums play for a drummer. */
  bandDrums: BandDrums;
  /** Pads and pedals on the kit that control the band. */
  kitTriggers: KitTriggers;
  /** The progression loop a drummer jams over, by id; the style's first if it has none such. */
  progression: string | null;
  /** The open song's recording, if it was read from one. */
  recording: RecordingState;
  /** Songs with a recording play it instead of the band. */
  playRecording: boolean;
  /** The recording's drums are heard; only a separated drum stem can be muted. */
  recordingDrums: boolean;
  /** Hits judged against the recording in the current or last run; see `judgedHits`. */
  judgedCount: number;
  liveSource: LiveSourceState;
  liveTracker: LiveTracker;
  /** The chord heard in the live source now, and the ones before it. */
  liveChord: ChordNow;
  liveLearning: LiveLearning;
  /** The song kept that is recognised in the live source, if any. */
  liveFollow: LiveFollowed | null;
  /** How much a keys player playing along is asked to play; see ALONG_LEVELS. */
  alongLevel: AlongLevel;
  /** Playing the chords heard, or soloing over them. */
  alongMode: AlongMode;
  /** How much of the solo the player plays; see SOLO_LEVELS. */
  soloLevel: SoloLevel;
  /** Call and response while soloing. */
  soloCall: boolean;
  /** How much the player's taps drive the solo now, 0..1. */
  soloFeel: number;
  /** The live song's key, which the solo plays in. */
  soloKey: SongKey | null;
  /** Inputs to play along with; names may be missing until audio input is allowed. */
  liveDevices: LiveDevice[];
  /** How much later the player hears each source than midzer does, in ms, by `liveSourceKey`. */
  liveOffsets: Record<string, number>;
  /** Taps so far while syncing, or null when not syncing. */
  liveSyncTaps: number | null;
  /** Why the last sync didn't take. */
  liveSyncError: string | null;
  /** A tick for hits clearly off (or wrong chords) while playing along. */
  missSound: MissSound;
  /** How loud the song's tune plays under the band in Learn, 0 (not at all) to 1. */
  melodyVolume: number;
  /** The General MIDI program the player's keys sound with. */
  sound: number;
}

export const BAND_DRUMS_OPTIONS: { mode: BandDrums; label: string }[] = [
  { mode: 'off', label: 'Off' },
  { mode: 'percussion', label: 'Shaker' },
  { mode: 'click', label: 'Click' },
  { mode: 'full', label: 'Full kit' },
];

export const LISTEN_OPTIONS: { mode: ListenMode; label: string; hint: string }[] = [
  { mode: 'off', label: 'Off', hint: '' },
  { mode: 'notes', label: 'Single notes', hint: 'Sing, whistle or play a melody line.' },
  { mode: 'chords', label: 'Chords', hint: 'Strum a guitar; the band follows your chords.' },
];

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
  micGateDb: number;
  headphones: boolean;
  tempoFollow: boolean;
  instrument: Instrument;
  kit: KitSetup;
  kitSound: boolean;
  countIn: CountInChoice;
  bandDrums: BandDrums;
  kitTriggers: KitTriggers;
  progression: string | null;
  playRecording: boolean;
  recordingDrums: boolean;
  liveOffsets: Record<string, number>;
  missSound: MissSound;
  alongLevel: AlongLevel;
  alongMode: AlongMode;
  soloLevel: SoloLevel;
  soloCall: boolean;
  melodyVolume: number;
  /** The General MIDI program the player's keys sound with. */
  sound: number;
}

function readPrefs(): Prefs {
  const defaults: Prefs = {
    style: 0,
    idleMode: 'rest',
    tapOffsetMs: null,
    micGateDb: -50,
    headphones: false,
    tempoFollow: false,
    instrument: 'keys',
    kit: { preset: 'gm' },
    kitSound: false,
    countIn: 'hiHat',
    bandDrums: 'off',
    kitTriggers: {},
    progression: null,
    playRecording: true,
    recordingDrums: true,
    liveOffsets: {},
    missSound: 'soft',
    alongLevel: parseLevel(undefined),
    alongMode: 'chords',
    soloLevel: parseSoloLevel(undefined),
    soloCall: false,
    melodyVolume: 0.4,
    sound: 0,
  };
  try {
    const saved = JSON.parse(localStorage.getItem(PREFS_KEY) ?? '{}') as Partial<Prefs>;
    const style = Number.isInteger(saved.style) && STYLES[saved.style!] ? saved.style! : defaults.style;
    const idleMode = IDLE_OPTIONS.some((o) => o.mode === saved.idleMode) ? saved.idleMode! : defaults.idleMode;
    const tapOffsetMs = typeof saved.tapOffsetMs === 'number' ? saved.tapOffsetMs : null;
    const micGateDb = typeof saved.micGateDb === 'number' ? Math.min(-10, Math.max(-90, saved.micGateDb)) : defaults.micGateDb;
    const headphones = saved.headphones === true;
    const tempoFollow = saved.tempoFollow === true;
    const instrument = saved.instrument === 'drums' ? 'drums' : 'keys';
    const kit = parseKitSetup(saved.kit);
    const kitSound = saved.kitSound === true;
    const countIn = COUNT_IN_OPTIONS.some((o) => o.choice === saved.countIn) ? saved.countIn! : defaults.countIn;
    const bandDrums = BAND_DRUMS_OPTIONS.some((o) => o.mode === saved.bandDrums) ? saved.bandDrums! : defaults.bandDrums;
    const kitTriggers = parseKitTriggers(saved.kitTriggers);
    const progression = typeof saved.progression === 'string' ? saved.progression : null;
    const playRecording = saved.playRecording !== false;
    const recordingDrums = saved.recordingDrums !== false;
    const liveOffsets = Object.fromEntries(
      Object.entries(saved.liveOffsets ?? {}).filter((e): e is [string, number] => typeof e[1] === 'number' && Math.abs(e[1]) <= 1000),
    );
    const missSound = MISS_SOUND_OPTIONS.some((o) => o.value === saved.missSound) ? saved.missSound! : defaults.missSound;
    const alongLevel = parseLevel(saved.alongLevel);
    const alongMode = parseAlongMode(saved.alongMode);
    const soloLevel = parseSoloLevel(saved.soloLevel);
    const soloCall = saved.soloCall === true;
    const melodyVolume = typeof saved.melodyVolume === 'number' ? Math.min(1, Math.max(0, saved.melodyVolume)) : defaults.melodyVolume;
    const sound = parseSound(saved.sound);
    return { style, idleMode, tapOffsetMs, micGateDb, headphones, tempoFollow, instrument, kit, kitSound, countIn, bandDrums, kitTriggers, progression, playRecording, recordingDrums, liveOffsets, missSound, alongLevel, alongMode, soloLevel, soloCall, melodyVolume, sound };
  } catch {
    return defaults;
  }
}

function savePrefs(): void {
  const { idleMode, tapOffsetMs, micGateDb, headphones, tempoFollow, instrument, kit, kitSound, countIn, bandDrums, kitTriggers, progression, playRecording, recordingDrums, liveOffsets, missSound, alongLevel, alongMode, soloLevel, soloCall, melodyVolume, sound } = get();
  // With a song open the style belongs to the song; keep the Jam one.
  const style = get().song ? readPrefs().style : get().style;
  const prefs: Prefs = { style, idleMode, tapOffsetMs, micGateDb, headphones, tempoFollow, instrument, kit, kitSound, countIn, bandDrums, kitTriggers, progression, playRecording, recordingDrums, liveOffsets, missSound, alongLevel, alongMode, soloLevel, soloCall, melodyVolume, sound };
  try {
    localStorage.setItem(PREFS_KEY, JSON.stringify(prefs));
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
  midiSupported: isMidiSupported() || isNativeHost(),
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
    cued: false,
    energy: 0.4,
  },
  idleMode: prefs.idleMode,
  tapOffsetMs: prefs.tapOffsetMs,
  outputLatencyMs: 0,
  song: null,
  songDirty: false,
  songHistory: { undo: 0, redo: 0 },
  songNotes: [],
  songBar: null,
  songStartBar: 0,
  loop: null,
  hasTake: false,
  listenMode: 'off',
  micGateDb: prefs.micGateDb,
  headphones: prefs.headphones,
  micError: null,
  tempoFollow: prefs.tempoFollow,
  instrument: prefs.instrument,
  kit: prefs.kit,
  kitSound: prefs.kitSound,
  countIn: prefs.countIn,
  countInHits: 0,
  bandDrums: prefs.bandDrums,
  kitTriggers: prefs.kitTriggers,
  progression: prefs.progression,
  recording: { status: 'none' },
  playRecording: prefs.playRecording,
  recordingDrums: prefs.recordingDrums,
  judgedCount: 0,
  liveSource: { status: 'none' },
  liveTracker: { locked: false, confidence: 0, level: 0 },
  liveChord: { current: null, before: [], next: null },
  liveLearning: { analysing: null, offered: [], error: null },
  liveFollow: null,
  alongLevel: prefs.alongLevel,
  alongMode: prefs.alongMode,
  soloLevel: prefs.soloLevel,
  soloCall: prefs.soloCall,
  soloFeel: 0,
  soloKey: null,
  liveDevices: [],
  liveOffsets: prefs.liveOffsets,
  missSound: prefs.missSound,
  melodyVolume: prefs.melodyVolume,
  sound: prefs.sound,
  liveSyncTaps: null,
  liveSyncError: null,
}));

const set = useSession.setState;
const get = useSession.getState;

let engine: EngineHandle | null = null;
let midiAccess: MIDIAccess | null = null;
/** In the desktop app MIDI is native; otherwise Web MIDI through `midiAccess`. */
let nativeMidi: NativeMidi | null = null;
let recorder: Recorder | null = null;

// ---- Navigation -------------------------------------------------------------

export function goTo(screen: Screen): void {
  const from = get().screen;
  if (from === 'along' && screen !== from) leavePlayAlong();
  if (SONG_SCREENS.includes(from) && screen !== from) closeSong(screen);
  else set({ screen });
  if (screen === 'along' && from !== screen) enterPlayAlong();
  syncJamHarmony();
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
  syncJamHarmony();
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
    if (isNativeHost()) {
      const { startNativeEngine } = await import('@midzer/audio-web/native');
      const native = await startNativeEngine();
      nativeMidi = native.midi;
      engine = native;
    } else {
      engine = await startEngine('/wasm/midzer.wasm', '/wasm/hear.wasm');
    }
    engine.onEvent(onEngineEvent);
    engine.onLiveChords(onLiveChords);
    engine.onLiveSong(onLiveSong);
    engine.onLiveFollow(onLiveFollow);
    const { bpm, volume, split, muted, style, idleMode, tempoFollow, instrument, kit, kitSound, countIn, bandDrums, kitTriggers, sound } = get();
    set({ outputLatencyMs: engine.outputLatencyMs });
    engine.send({ type: 'setBandDrums', mode: bandDrums });
    engine.send({ type: 'setInstrument', instrument });
    engine.send({ type: 'setSound', program: sound });
    sendSolo();
    for (const cmd of kitCommands(kitMap(kit))) engine.send(cmd);
    for (const cmd of triggerCommands(kitTriggers)) engine.send(cmd);
    engine.send({ type: 'setKitSound', on: kitSound });
    engine.send({ type: 'setCountIn', pieces: countInPieces(countIn) });
    engine.send({ type: 'setStyle', index: style });
    engine.send({ type: 'setTempoFollow', on: tempoFollow });
    engine.send({ type: 'setIdleMode', mode: idleMode });
    sendLatency();
    engine.send({ type: 'setTempo', bpm });
    engine.send({ type: 'setGain', gain: volume });
    engine.send({ type: 'setSplit', note: split });
    recorder = new Recorder(engine.sampleRate);
    syncSong();
    syncMelody();
    void loadSongRecording();
    syncJamHarmony();
    if (get().screen === 'along') enterPlayAlong();
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
  if ((event.type === 'noteOn' || event.type === 'noteOff') && event.channel === MIC_CHANNEL) {
    // Heard by the mic: it was played before the engine noticed, not when this is heard.
    const playedMs = timeMs - get().outputLatencyMs - LISTEN_DELAY_MS[get().listenMode];
    notifyPlayer(event.type === 'noteOn' ? { type: 'noteOn', note: event.key, velocity: event.velocity } : { type: 'noteOff', note: event.key }, playedMs);
  }
  enqueue(event, timeMs);
  sendMidiOut(event, timeMs);
  recorder?.record(event);
  for (const listener of engineListeners) listener(event, timeMs);

  // UI state changes when the change is heard, not when it is scheduled. A
  // live source isn't heard through midzer's output, so without its latency.
  const heardMs = get().screen === 'along' ? timeMs - get().outputLatencyMs : timeMs;
  const delay = Math.max(0, heardMs - performance.now());
  switch (event.type) {
    case 'chord':
      setTimeout(() => set({ chord: event.chord }), delay);
      break;
    case 'beat':
      live.beatClock.ms = heardMs;
      live.beatClock.beat = event.beat;
      setTimeout(() => set({ bar: event.bar, beat: event.beat }), delay);
      break;
    case 'transport':
      if (event.playing) {
        live.timing.reset();
        live.hits.reset();
        skipJudged = false;
        judged = [];
        set({ judgedCount: 0 });
      }
      setTimeout(
        () =>
          set({
            playing: event.playing,
            hasTake: recorder?.hasTake ?? false,
            countInHits: 0,
            ...(event.playing ? {} : { songBar: null }),
          }),
        delay,
      );
      break;
    case 'timing':
      live.timing.push(event.ms);
      break;
    case 'countIn':
      set({ countInHits: event.hits });
      break;
    case 'judged':
    case 'keysJudged': {
      if (skipJudged) break;
      const chord = event.type === 'keysJudged' ? event.chord : null;
      judged.push({ ms: event.ms, piece: event.type === 'keysJudged' ? 'keys' : event.piece, chord, sixteenth: event.sixteenth, bar: event.bar });
      set({ judgedCount: judged.length });
      if (syncTaps) {
        syncTap(event.ms, event.sixteenth);
        break;
      }
      live.hits.push(event.ms, chord);
      const miss = missOf(event.ms, chord);
      if (miss) playMiss(miss, get().missSound);
      break;
    }
    case 'live':
      set({ liveTracker: { locked: event.locked, confidence: event.confidence, level: event.level } });
      break;
    case 'liveForgot':
      forgetLiveSong();
      break;
    case 'soloFeel':
      setTimeout(() => set({ soloFeel: event.energy }), delay);
      break;
    case 'songPosition': {
      live.songClock.ms = heardMs;
      live.songClock.beat = event.beat;
      const at = barAt(startsOf(get().song?.bars), event.beat);
      if (at.beat === 0) setTimeout(() => set({ songBar: at.bar }), delay);
      break;
    }
    case 'band':
      setTimeout(() => set({ band: event.state }), delay);
      break;
    case 'tempo':
      setTimeout(() => set({ bpm: Math.round(event.bpm) }), delay);
      break;
    case 'input':
      live.mic.level = event.level;
      live.mic.key = event.key ?? -1;
      break;
  }
  for (const listener of beatListeners) if (event.type === 'beat') listener(event.bar, event.beat, timeMs);
}

type EngineListener = (event: TimedEvent, timeMs: number) => void;
const engineListeners = new Set<EngineListener>();

/** Calls `listener` with every engine event and the time it is heard. Returns an unsubscribe function. */
export function onEngineEvents(listener: EngineListener): () => void {
  engineListeners.add(listener);
  return () => engineListeners.delete(listener);
}

type PlayerListener = (note: number, on: boolean, timeMs: number) => void;
const playerListeners = new Set<PlayerListener>();

/**
 * Calls `listener` for each key the player presses or releases, from any
 * input, with the time it was played. Returns an unsubscribe function.
 */
export function onPlayerNote(listener: PlayerListener): () => void {
  playerListeners.add(listener);
  return () => playerListeners.delete(listener);
}

function notifyPlayer(cmd: EngineCommand, timeMs = performance.now()): void {
  if (cmd.type === 'noteOn' || cmd.type === 'noteOff') {
    const on = cmd.type === 'noteOn' && cmd.velocity > 0;
    for (const listener of playerListeners) listener(cmd.note, on, timeMs);
  }
}

type DrumInputListener = (note: number, velocity: number) => void;
const drumInputListeners = new Set<DrumInputListener>();

/**
 * Calls `listener` with each note a drum source sends, before the kit map:
 * notes on the MIDI drum channel, and every note while playing drums.
 * Returns an unsubscribe function.
 */
export function onDrumInput(listener: DrumInputListener): () => void {
  drumInputListeners.add(listener);
  return () => drumInputListeners.delete(listener);
}

/** Tells the UI about input from a MIDI device, which the engine already has. */
function notifyInput(cmd: EngineCommand): void {
  const drums = get().instrument === 'drums';
  if (cmd.type === 'drumNote' || (drums && cmd.type === 'noteOn')) {
    for (const listener of drumInputListeners) listener(cmd.note, cmd.velocity);
  } else if (cmd.type === 'hiHatPedal') {
    live.hiHat = cmd.value;
  } else if (cmd.type === 'controlChange') {
    for (const listener of controlListeners) listener(cmd.cc, cmd.value);
  }
  if (!drums) notifyPlayer(cmd);
}

type ControlListener = (cc: number, value: number) => void;
const controlListeners = new Set<ControlListener>();

/** Calls `listener` with each MIDI controller change from a device. Returns an unsubscribe function. */
export function onControlChange(listener: ControlListener): () => void {
  controlListeners.add(listener);
  return () => controlListeners.delete(listener);
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

/**
 * Picks a style. Stopped, it also takes the style's tempo; playing, it changes
 * on the next bar. With a song open the style belongs to the song.
 */
export function setStyle(index: number): void {
  const style = STYLES[index];
  if (!style) return;
  set({ style: index });
  send({ type: 'setStyle', index });
  const { song, screen, playing } = get();
  if (song) {
    if (screen === 'song') editSong({ ...song, style: index });
    return;
  }
  savePrefs();
  if (!playing) setTempo(style.bpm);
  syncJamHarmony();
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

export function setMissSound(missSound: MissSound): void {
  set({ missSound });
  savePrefs();
  // Let the player hear what they picked.
  playMiss('early', missSound);
}

/** How loud the song's tune plays under the band in Learn. */
export function setMelodyVolume(melodyVolume: number): void {
  set({ melodyVolume: Math.min(1, Math.max(0, melodyVolume)) });
  savePrefs();
  sendMelodyVolume();
}

/** The tune is heard in Learn only. */
function sendMelodyVolume(): void {
  send({ type: 'setMelody', volume: get().screen === 'lesson' ? get().melodyVolume : 0 });
}

export function setAlongLevel(alongLevel: AlongLevel): void {
  set({ alongLevel });
  savePrefs();
}

/** Play the chords heard, or solo over them. */
export function setAlongMode(alongMode: AlongMode): void {
  set({ alongMode });
  savePrefs();
  sendSolo();
}

export function setSoloLevel(soloLevel: SoloLevel): void {
  set({ soloLevel });
  savePrefs();
  sendSolo();
}

export function setSoloCall(soloCall: boolean): void {
  set({ soloCall });
  savePrefs();
  sendSolo();
}

/** Whether the player's keys drive a solo now. */
function soloing(): boolean {
  const { alongMode, instrument } = get();
  return alongMode === 'solo' && instrument === 'keys';
}

/** The engine solos only over live audio, so this can stay on elsewhere. */
function sendSolo(): void {
  const { soloLevel, soloCall, outputLatencyMs } = get();
  send({ type: 'setSolo', level: soloing() ? soloLevel : 0, call: soloCall, leadMs: outputLatencyMs });
  if (!soloing()) set({ soloFeel: 0 });
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

/** The band speeds up and slows down with the player, within 15 % of the set tempo. */
export function setTempoFollow(on: boolean): void {
  set({ tempoFollow: on });
  savePrefs();
  send({ type: 'setTempoFollow', on });
}

// ---- Drums --------------------------------------------------------------------

export function setInstrument(instrument: Instrument): void {
  set({ instrument, countInHits: 0 });
  savePrefs();
  send({ type: 'setInstrument', instrument });
  resetLive();
  syncJamHarmony();
  sendSolo();
}

/** Which pieces count the band in. */
export function setCountIn(countIn: CountInChoice): void {
  set({ countIn, countInHits: 0 });
  savePrefs();
  send({ type: 'setCountIn', pieces: countInPieces(countIn) });
}

/** What the band's own drums play while the player drums. */
export function setBandDrums(mode: BandDrums): void {
  set({ bandDrums: mode });
  savePrefs();
  send({ type: 'setBandDrums', mode });
}

/** Sets which pad or pedal does `action`; null clears it. */
export function setKitTrigger(action: KitAction, trigger: KitTrigger | null): void {
  const { triggers, commands } = assignTrigger(get().kitTriggers, action, trigger);
  set({ kitTriggers: triggers });
  savePrefs();
  for (const cmd of commands) send(cmd);
}

/** The progression loop a drummer jams over in the current style. */
export function currentProgression() {
  return pickProgression(get().style, get().progression);
}

export function setProgression(id: string): void {
  set({ progression: id });
  savePrefs();
  syncJamHarmony();
}

/**
 * Drums form no chords, so in Jam a drummer plays over a progression loop
 * through song mode; a keyboardist's own chords set the harmony. A song
 * screen loads its own chart.
 */
function syncJamHarmony(): void {
  const { screen, instrument, song, playing } = get();
  if (song) return;
  if (screen === 'jam' && instrument === 'drums') {
    for (const cmd of progressionCommands(currentProgression())) send(cmd);
    if (!playing) send({ type: 'songSeek', bar: 0 });
    send({ type: 'setSong', on: true });
  } else {
    send({ type: 'setSong', on: false });
  }
}

/** Uses a preset or a learned note map for the e-kit. */
export function setKit(kit: KitSetup): void {
  set({ kit });
  savePrefs();
  for (const cmd of kitCommands(kitMap(kit))) send(cmd);
}

/** The sound the player's keys play. */
export function setSound(program: number): void {
  set({ sound: program });
  savePrefs();
  send({ type: 'setSound', program });
}

export function setKitSound(on: boolean): void {
  set({ kitSound: on });
  savePrefs();
  send({ type: 'setKitSound', on });
}

/** A hit from the on-screen or computer-keyboard pads. */
export function drumHit(piece: DrumPiece, velocity = 100): void {
  send({ type: 'drumHit', piece, velocity });
}

// ---- Microphone ---------------------------------------------------------------

let closeMic: (() => void) | null = null;

/** Opens the microphone (asking permission) or closes it. */
export async function setListenMode(mode: ListenMode): Promise<void> {
  if (!engine) return;
  set({ listenMode: mode, micError: null });
  if (mode === 'off') {
    stopMic();
    return;
  }
  if (!closeMic) {
    try {
      closeMic = await engine.openMic({ echoCancellation: !get().headphones });
    } catch (e) {
      set({ listenMode: 'off', micError: micErrorText(e) });
      return;
    }
  }
  if (get().listenMode !== mode) return; // changed while asking for permission
  send({ type: 'setListen', mode, gateDb: get().micGateDb });
}

function stopMic(): void {
  send({ type: 'setListen', mode: 'off', gateDb: get().micGateDb });
  closeMic?.();
  closeMic = null;
  live.mic.level = 0;
  live.mic.key = -1;
}

function micErrorText(e: unknown): string {
  const name = e instanceof DOMException ? e.name : '';
  if (name === 'NotAllowedError') return 'Microphone access was blocked. Allow it in the browser to listen.';
  if (name === 'NotFoundError') return 'No microphone was found.';
  return e instanceof Error ? e.message : String(e);
}

export function setMicGate(db: number): void {
  set({ micGateDb: db });
  savePrefs();
  const { listenMode } = get();
  if (listenMode !== 'off') send({ type: 'setListen', mode: listenMode, gateDb: db });
}

/** Headphones turn echo cancellation off; an open mic reopens with the new setting. */
export function setHeadphones(on: boolean): void {
  set({ headphones: on });
  savePrefs();
  const { listenMode } = get();
  if (closeMic) {
    stopMic();
    void setListenMode(listenMode);
  }
}

// ---- Songs ------------------------------------------------------------------

let starts: { bars: Bar[] | undefined; starts: number[] } = { bars: undefined, starts: [0] };

/** Where the bars of `bars` start, kept for the chart in use; without one (Jam), bars of four. */
function startsOf(bars: Bar[] | undefined): number[] {
  if (starts.bars !== bars) starts = { bars, starts: bars ? barStarts(bars) : [0] };
  return starts.starts;
}

/** Loads the open song's chart, loop and start into the engine. */
function syncSong(): void {
  const { song, loop, songStartBar } = get();
  if (!song) return;
  for (const cmd of chartCommands(song)) send(cmd);
  send({ type: 'songLoop', start: loop?.start ?? 0, end: loop?.end ?? 0 });
  if (!get().playing) send({ type: 'songSeek', bar: songStartBar });
  send({ type: 'setSong', on: true });
}

/** Opens a song: the band follows its chart, style and tempo (scaled by `speed`). */
export function openSong(song: Song, notes: string[] = [], { screen = 'song' as Screen, speed = 1 } = {}): void {
  send({ type: 'stop' });
  send({ type: 'songWait', tick: null });
  unloadRecording();
  clearHistory();
  set({ song, songDirty: false, songNotes: notes, songBar: null, songStartBar: 0, loop: null, screen, style: song.style });
  send({ type: 'setStyle', index: song.style });
  setTempo(song.bpm * speed);
  syncSong();
  syncMelody();
  void loadSongRecording();
}

/** Loads the open song's tune into the engine, at the volume for the screen. */
function syncMelody(): void {
  for (const cmd of melodyCommands(get().song?.melody)) send(cmd);
  sendMelodyVolume();
}

/** Leaves the song: back to following the player, with the Jam style and tempo. */
export function closeSong(screen: Screen = 'songs'): void {
  send({ type: 'stop' });
  send({ type: 'songWait', tick: null });
  send({ type: 'setSong', on: false });
  unloadRecording();
  clearHistory();
  const { style } = readPrefs();
  set({ song: null, songDirty: false, songBar: null, loop: null, screen, style });
  sendMelodyVolume();
  send({ type: 'setStyle', index: style });
  setTempo(STYLES[style]!.bpm);
  syncJamHarmony();
}

/** Edits of the open song that can be undone, oldest first, and those undone that can be redone. */
let undoStack: Song[] = [];
let redoStack: Song[] = [];
/** What the last edit changed, so typing into one field is undone as one step. */
let lastEditKind: string | null = null;
const MAX_UNDO = 100;

function clearHistory(): void {
  undoStack = [];
  redoStack = [];
  lastEditKind = null;
  set({ songHistory: { undo: 0, redo: 0 } });
}

/**
 * Changes the open song; the band hears the new chart from the next beat and
 * a playing recording the new beat map. Each edit can be undone; edits of the
 * same `kind` in a row (typing a title) are undone as one.
 */
export function editSong(song: Song, kind?: string): void {
  const before = get().song;
  if (before && (!kind || kind !== lastEditKind)) {
    undoStack = [...undoStack, before].slice(-MAX_UNDO);
    redoStack = [];
  }
  lastEditKind = kind ?? null;
  showSong(song, before);
}

/** Puts `song` in place of the open one (`before`), unsaved. */
function showSong(song: Song, before: Song | null): void {
  const loop = get().loop;
  const bars = song.bars.length;
  set({
    song,
    songDirty: true,
    songHistory: { undo: undoStack.length, redo: redoStack.length },
    loop: loop && loop.start < bars ? { start: loop.start, end: Math.min(loop.end, bars) } : null,
    songStartBar: Math.min(get().songStartBar, Math.max(0, bars - 1)),
  });
  syncSong();
  if (song.melody !== before?.melody) syncMelody();
  if (song.bpm !== before?.bpm) setTempo(song.bpm);
  if (song.backing && song.backing.beatTimes !== before?.backing?.beatTimes && get().recording.status === 'ready') {
    for (const cmd of trackCommands(song.backing)) send(cmd);
  }
}

/** Takes the open song's last edit back, chart and timeline alike. */
export function undoSong(): void {
  const song = get().song;
  const previous = undoStack[undoStack.length - 1];
  if (!song || !previous) return;
  undoStack = undoStack.slice(0, -1);
  redoStack = [...redoStack, song];
  lastEditKind = null;
  showSong(previous, song);
}

export function redoSong(): void {
  const song = get().song;
  const next = redoStack[redoStack.length - 1];
  if (!song || !next) return;
  redoStack = redoStack.slice(0, -1);
  undoStack = [...undoStack, song];
  lastEditKind = null;
  showSong(next, song);
}

/** The level of the open song over time (100 a second), for the timeline; read from its recording once if it was imported before songs kept it. */
export async function songEnvelope(): Promise<Uint8Array | null> {
  const song = get().song;
  if (!song) return null;
  const kept = song.backing?.envelope ?? song.heard?.envelope;
  if (kept?.length) return kept;
  if (!song.backing) return null;
  const envelope = await recordingEnvelope(song.id);
  if (!envelope) return null;
  // Kept with the song as saved, without touching unsaved edits.
  const saved = (await loadUserSongs()).find((s) => s.id === song.id);
  if (saved?.backing) void saveUserSong({ ...saved, backing: { ...saved.backing, envelope } });
  return envelope;
}

/** Saves the open song to My songs; a built-in one is saved as a copy. Resolves false on failure. */
export async function saveSong(): Promise<boolean> {
  const { song } = get();
  if (!song) return false;
  const saved = isBuiltin(song) ? { ...song, id: newSongId(), source: 'user' as const, title: `${song.title} (my version)` } : song;
  if (!(await saveUserSong(saved))) return false;
  if (get().song === song) set({ song: saved, songDirty: false });
  // The listener follows the map as fixed from now on.
  if (get().liveSource.status === 'on') void sendLiveLibrary();
  return true;
}

/** Stopped: the song starts here. Playing: it jumps here at the next bar line. */
export function seekSong(bar: number): void {
  set({ songStartBar: bar });
  send({ type: 'songSeek', bar });
}

export function setLoop(loop: LoopRange | null): void {
  set({ loop });
  send({ type: 'songLoop', start: loop?.start ?? 0, end: loop?.end ?? 0 });
}

// ---- Playing along with a recording ----------------------------------------------

/** Hits judged against the recording, in the order played. */
let judged: JudgedHit[] = [];
/** Counts loads, so one that finishes after the song changed is dropped. */
let recordingRequest = 0;

export function judgedHits(): readonly JudgedHit[] {
  return judged;
}

/** Reads the open song's recording into the engine, with its tempo map, when it has one. */
async function loadSongRecording(): Promise<void> {
  const { song, screen } = get();
  const request = ++recordingRequest;
  const backing = song?.backing;
  if (!engine || !song || !backing || screen !== 'song') return;
  const current = () => request === recordingRequest;
  set({ recording: { status: 'loading' } });
  try {
    const bytes = await loadRecording(song.id);
    if (!bytes) throw new Error('The recording is no longer kept on this device. Import it again to play along with it.');
    const source: TrackSource = isNativeHost()
      ? { kind: 'file', bytes, stemsDir: backing.stemsDir ?? null }
      : { kind: 'samples', main: await decodeRecording(bytes, engine.sampleRate), drums: null };
    if (!current()) return;
    const loaded = await engine.loadTrack(source);
    if (!current()) {
      // Closed meanwhile; a newer load replaces it anyway.
      if (get().recording.status !== 'loading') void engine.loadTrack(null);
      return;
    }
    for (const cmd of trackCommands(backing)) send(cmd);
    send({ type: 'setTrackDrums', on: get().recordingDrums });
    send({ type: 'setTrack', on: get().playRecording });
    set({ recording: loaded ? { status: 'ready', ...loaded } : { status: 'none' } });
  } catch (e) {
    if (current()) set({ recording: { status: 'error', message: e instanceof Error ? e.message : String(e) } });
  }
}

/**
 * Reads the open song's stored recording again and replaces what was read
 * from it: the beat map, tempo, chords and sections, and a melody guessed
 * from the mix. Title, style, lyrics where the bars line up, separated stems
 * and their parts stay. With `keepFixes`, what was set by hand in the
 * timeline or the chart stays too (see `keepFixes` in timeline.ts); without,
 * it goes. The song is then unsaved, like any edit.
 */
export async function analyzeSongAgain(onProgress: (fraction: number, step: number) => void, signal?: AbortSignal, keep = false): Promise<void> {
  const song = get().song;
  if (!song?.backing) throw new Error('This song was not read from a recording.');
  const bytes = await loadRecording(song.id);
  if (!bytes) throw new Error('The recording is no longer kept on this device. Import it again to read it again.');
  const analysis = await analyzeAudio(bytes, { stems: false, onProgress, signal });
  const current = get().song;
  if (signal?.aborted || current?.id !== song.id || !current.backing) return;
  const { song: fresh, notes, fingerprint } = songFromAnalysis(analysis, current.title);
  void saveFingerprint(song.id, fingerprint);
  const old = timelineOf(current);
  let next: Song = {
    ...current,
    bpm: fresh.bpm,
    bars: fresh.bars.map((b, i) => (current.bars[i]?.lyric ? { ...b, lyric: current.bars[i]!.lyric } : b)),
    backing: { ...current.backing, ...fresh.backing },
  };
  delete next.fixes;
  if (keep && old && current.fixes && fresh.backing) next = withTimeline(next, keepFixes(old, { bars: fresh.bars, beatTimes: fresh.backing.beatTimes }));
  // A melody from the separated vocals beats one guessed from the mix.
  if (!current.backing.stemsDir) {
    if (fresh.melody) next.melody = fresh.melody;
    else delete next.melody;
  }
  editSong(next);
  set({ songNotes: notes });
}

function unloadRecording(): void {
  recordingRequest++;
  send({ type: 'setTrack', on: false });
  if (get().recording.status !== 'none') void engine?.loadTrack(null);
  judged = [];
  set({ recording: { status: 'none' }, judgedCount: 0 });
}

/** Whether songs with a recording play it (true) or the band (false). */
export function setPlayRecording(on: boolean): void {
  set({ playRecording: on });
  savePrefs();
  if (get().recording.status === 'ready') send({ type: 'setTrack', on });
}

/** Mutes or unmutes the recording's separated drums. */
export function setRecordingDrums(on: boolean): void {
  set({ recordingDrums: on });
  savePrefs();
  send({ type: 'setTrackDrums', on });
}

/** Whether the open song plays its recording right now. */
export function playsRecording(): boolean {
  const { recording, playRecording, screen } = get();
  return screen === 'song' && recording.status === 'ready' && playRecording;
}

// ---- Playing along with live audio ---------------------------------------------

let closeLiveSource: (() => void) | null = null;
const liveChords = new ChordTimeline();
let liveChordTimer: ReturnType<typeof setInterval> | undefined;

/** The listener's chords, moved to when the player hears them: like the live beats, without midzer's output latency. */
function onLiveChords(spans: LiveChordSpan[]): void {
  const { liveSource, liveOffsets, outputLatencyMs } = get();
  if (liveSource.status !== 'on') return;
  liveChords.add(spans, (liveOffsets[liveSource.key] ?? 0) - outputLatencyMs);
}

/** The live source's chords between `fromMs` and `toMs` in the player's time, for the moving chord view. */
export function liveChordWindow(fromMs: number, toMs: number, nowMs: number): HeardChord[] {
  return liveChords.window(fromMs, toMs, nowMs);
}

/** The keys lit for a keys player, so they are only redrawn when they change. */
let alongHinted = '';

function setAlongHints(hints: Map<number, number>): void {
  const key = [...hints].join();
  if (key === alongHinted) return;
  alongHinted = key;
  live.hint.fill(0);
  for (const [k, h] of hints) live.hint[k] = h;
  live.version++;
}

/** The live song's key, kept steady for the solo. */
const liveKey = new KeyFollower();
/** The harmony last sent to the solo. */
let soloHarmonySent = '';

/** The chords heard over this long tell the key. */
const KEY_WINDOW_MS = 30_000;
/** The solo moves to the next chord this much before it is heard, as the lit keys do. */
const SOLO_SWITCH_MS = 150;

/** Tells the solo the song's key and the chord heard now; returns the key. */
function sendSoloHarmony(spans: readonly HeardChord[], ms: number): SongKey | null {
  const weighted: WeightedChord[] = spans.flatMap((s) => (s.chord && !s.predicted ? [{ chord: s.chord, weight: Math.min(s.toMs, ms) - s.fromMs }] : [])).filter((c) => c.weight > 0);
  const key = get().liveFollow && liveKey.key ? liveKey.key : liveKey.update(weighted, ms);
  const now = spans.find((s) => s.fromMs <= ms + SOLO_SWITCH_MS && ms + SOLO_SWITCH_MS < s.toMs)?.chord ?? null;
  if (key) {
    const harmony = { type: 'soloHarmony', tonic: key.tonic, scale: pentatonic(key), chord: chordBits(now) } as const;
    const sent = `${harmony.tonic}/${harmony.scale}/${harmony.chord}`;
    if (sent !== soloHarmonySent) {
      soloHarmonySent = sent;
      send(harmony);
    }
  }
  const shown = get().soloKey;
  if (key?.tonic !== shown?.tonic || key?.minor !== shown?.minor) set({ soloKey: key });
  return key;
}

/** The source went quiet or the player started over: what was heard of the song goes. */
function forgetLiveSong(): void {
  liveKey.reset();
  soloHarmonySent = '';
  set({ soloKey: null });
  liveChords.clear();
  set({ liveChord: { current: null, before: [], next: null } });
  setAlongHints(new Map());
  resetLive();
  set({ liveFollow: null });
}

// ---- Recognising songs kept ---------------------------------------------------------

const liveIds = new LiveIds();
/** The songs kept that the listener can recognise, by id, as last sent. */
let recognisable = new Map<string, Song>();
let libraryRequest = 0;

/** Hands the listener the songs kept that it can recognise (with a fingerprint and a beat map). */
async function sendLiveLibrary(): Promise<void> {
  if (!engine) return;
  const request = ++libraryRequest;
  const songs = await loadUserSongs();
  const entries: LiveLibrarySong[] = [];
  const found = new Map<string, Song>();
  for (const song of songs) {
    const entry = liveLibrarySong(liveIds.of(song.id), song, await loadFingerprint(song.id));
    if (!entry) continue;
    entries.push(entry);
    found.set(song.id, song);
  }
  if (request !== libraryRequest || !engine) return;
  recognisable = found;
  engine.setLiveLibrary(entries);
}

function onLiveFollow(follow: LiveFollow | null): void {
  const song = follow ? recognisable.get(liveIds.songOf(follow.id) ?? '') : undefined;
  set({ liveFollow: song ? { songId: song.id, title: song.title, bpm: song.bpm, chords: chordsUsed(song.bars) } : null });
  // A song kept knows its key from its whole chart.
  const key = song ? keyOf(chartChords(song.bars)) : null;
  if (key) liveKey.set(key);
}

/** A chart's chords, each weighted by its beats. */
function chartChords(bars: Bar[]): WeightedChord[] {
  return resolvedBeats(bars).flatMap((name) => {
    const spec = name ? parseChord(name) : null;
    return spec ? [{ chord: { root: spec.root, quality: spec.quality }, weight: 1 }] : [];
  });
}

/** "Not this song": stops following the song recognised until the music stops or starts over. */
export function notThisSong(): void {
  const followed = get().liveFollow;
  if (!engine || !followed) return;
  engine.ignoreLiveSong(liveIds.of(followed.songId));
  set({ liveFollow: null });
}

// ---- Learning songs heard live ---------------------------------------------------

/** At most this many learned songs wait to be kept. */
const MAX_OFFERED = 5;
/** The next song to analyse (and the song kept in part it is more of); a newer one replaces it. */
let nextCapture: { capture: LiveSongCapture; extends?: Song } | null = null;
let analysingCapture = false;

/** Whether songs played along with can be learned here. */
export function canLearnLiveSongs(): boolean {
  return engine?.canLearnSongs ?? false;
}

/** Ends the live song here and learns it (the "Save this song" button). */
export function saveLiveSong(): void {
  engine?.finishLiveSong();
}

function onLiveSong(capture: LiveSongCapture): void {
  const followed = capture.followed === undefined ? undefined : recognisable.get(liveIds.songOf(capture.followed) ?? '');
  const fate = captureFate(followed, capture);
  // A song kept already, heard again.
  if (fate === 'known') return;
  nextCapture = fate === 'more' && followed ? { capture, extends: followed } : { capture };
  if (!analysingCapture) void learnNext();
}

const learning = () => get().liveLearning;

async function learnNext(): Promise<void> {
  const next = nextCapture;
  nextCapture = null;
  if (!next) return;
  const { capture, extends: more } = next;
  analysingCapture = true;
  const seconds = capture.seconds;
  set({ liveLearning: { ...learning(), analysing: { fraction: 0, seconds }, error: null } });
  try {
    const analysis = await analyzeCapture(capture, {
      onProgress: (fraction) => set({ liveLearning: { ...learning(), analysing: { fraction, seconds } } }),
    });
    const heardAt = Date.now() - seconds * 1000;
    const when = new Date(heardAt).toLocaleString(undefined, { month: 'short', day: 'numeric', hour: '2-digit', minute: '2-digit' });
    const learned: LearnedSong = songFromAnalysis(analysis, more?.title ?? `Heard ${when}`, { complete: capture.complete, heardAt });
    if (more) learned.extends = { id: more.id, title: more.title };
    set({ liveLearning: { ...learning(), analysing: null, offered: [learned, ...learning().offered].slice(0, MAX_OFFERED) } });
  } catch (e) {
    set({ liveLearning: { ...learning(), analysing: null, error: e instanceof Error ? e.message : String(e) } });
  } finally {
    analysingCapture = false;
    if (nextCapture) void learnNext();
  }
}

/**
 * Keeps a learned song in My songs, with its fingerprint, under the name
 * given (blank keeps the one it was offered with); more of a song kept in
 * part replaces that song's chart and beat map, keeping its title, artist
 * and style. Resolves false if it could not be stored.
 */
export async function keepLearnedSong(id: string, title?: string): Promise<boolean> {
  const learned = learning().offered.find((l) => l.song.id === id);
  if (!learned) return false;
  const kept = learned.extends ? (await loadUserSongs()).find((s) => s.id === learned.extends!.id) : undefined;
  const named = title?.trim();
  const song: Song = kept
    ? { ...learned.song, id: kept.id, title: kept.title, style: kept.style, ...(kept.artist ? { artist: kept.artist } : {}) }
    : named
      ? { ...learned.song, title: named }
      : learned.song;
  if (!(await saveUserSong(song))) return false;
  await saveFingerprint(song.id, learned.fingerprint);
  dismissLearnedSong(id);
  if (get().liveSource.status === 'on') void sendLiveLibrary();
  return true;
}

export function dismissLearnedSong(id: string): void {
  set({ liveLearning: { ...learning(), offered: learning().offered.filter((l) => l.song.id !== id) } });
}

export function clearLearnError(): void {
  set({ liveLearning: { ...learning(), error: null } });
}

/** Starts the live song over: the engine and the listener forget it and listen afresh. */
export function resetLiveSource(): void {
  if (!engine || get().liveSource.status !== 'on') return;
  engine.resetLive();
  forgetLiveSong();
}

/** Shows the chord heard now, a few times a second, while a source is followed, and lights its keys for a keys player. */
function followLiveChords(on: boolean): void {
  clearInterval(liveChordTimer);
  forgetLiveSong();
  if (!on) return;
  liveChordTimer = setInterval(() => {
    const ms = performance.now();
    const { instrument, alongLevel, bpm, liveTracker, soloLevel } = get();
    // The next chord's keys show from a beat before it.
    const leadMs = liveTracker.locked && bpm > 0 ? 60_000 / bpm : 600;
    const spans = liveChords.window(ms - KEY_WINDOW_MS, ms + leadMs + 1, ms);
    // On the beats the player sees, like the falling chords.
    const snapped = liveTracker.locked && bpm > 0 ? snapToBeats(spans, live.beatClock.ms, 60_000 / bpm) : spans;
    const key = sendSoloHarmony(snapped, ms);
    if (soloing()) setAlongHints(soloHints(soloLevel, key));
    else setAlongHints(instrument === 'keys' ? keyHints(snapped, ms, alongLevel, leadMs) : new Map());
    const now = liveChords.at(ms);
    const shown = get().liveChord;
    // The countdown to the next chord changes every tenth of a second, not on every tick.
    const nextKey = (c: typeof now) => (c.next ? `${c.next.name}@${Math.ceil(c.next.inMs / 100)}` : '');
    if (now.current !== shown.current || now.before.join() !== shown.before.join() || nextKey(now) !== nextKey(shown)) set({ liveChord: now });
  }, 50);
}
/** Counts opens, so one that finishes after another started is dropped. */
let liveRequest = 0;
/** Taps on the beat while syncing. */
let syncTaps: number[] | null = null;
/** After syncing, the rest of that run's hits (still being judged) aren't drumming either. */
let skipJudged = false;

/** Where a source's sync offset is kept. */
export function liveSourceKey(source: LiveSource): string {
  return source.kind === 'tab' ? 'tab' : `device:${source.id}`;
}

/** Whether the engine can play along with a browser tab here. */
export function canCaptureTab(): boolean {
  return engine?.canCaptureTab ?? false;
}

function enterPlayAlong(): void {
  if (!engine) return;
  send({ type: 'stop' });
  send({ type: 'setLive', on: true });
  judged = [];
  set({ judgedCount: 0, liveTracker: { locked: false, confidence: 0, level: 0 } });
  void refreshLiveDevices();
}

function leavePlayAlong(): void {
  stopLiveSource();
  syncTaps = null;
  set({ liveSyncTaps: null });
  send({ type: 'setLive', on: false });
}

export async function refreshLiveDevices(): Promise<void> {
  if (!engine) return;
  try {
    set({ liveDevices: await engine.liveDevices() });
  } catch (e) {
    console.warn('midzer: no audio inputs listed', e);
  }
}

/** Listens to `source` and follows its beat; the browser may ask the user to pick or allow it. */
export async function startLiveSource(source: LiveSource): Promise<void> {
  if (!engine) return;
  stopLiveSource();
  const request = ++liveRequest;
  const key = liveSourceKey(source);
  set({ liveSource: { status: 'opening', key } });
  try {
    const close = await engine.openLive(source, () => {
      if (closeLiveSource !== close) return;
      closeLiveSource = null;
      send({ type: 'stop' });
      set({ liveSource: { status: 'ended', key } });
      followLiveChords(false);
    });
    if (request !== liveRequest) {
      close();
      return;
    }
    closeLiveSource = close;
    // Listening starts afresh: the old source's beat means nothing here.
    send({ type: 'setLive', on: false });
    send({ type: 'setLiveOffset', ms: get().liveOffsets[key] ?? 0 });
    send({ type: 'setLive', on: true });
    set({ liveSource: { status: 'on', key }, liveTracker: { locked: false, confidence: 0, level: 0 } });
    followLiveChords(true);
    void sendLiveLibrary();
    // Names show once audio input is allowed.
    if (source.kind === 'device') void refreshLiveDevices();
  } catch (e) {
    if (request !== liveRequest) return;
    set({ liveSource: { status: 'error', key, message: liveErrorText(e) } });
  }
}

export function stopLiveSource(): void {
  liveRequest++;
  closeLiveSource?.();
  closeLiveSource = null;
  if (get().liveSource.status !== 'none') {
    send({ type: 'stop' });
    set({ liveSource: { status: 'none' }, liveTracker: { locked: false, confidence: 0, level: 0 } });
  }
  followLiveChords(false);
}

function liveErrorText(e: unknown): string {
  const name = e instanceof DOMException ? e.name : '';
  if (name === 'NotAllowedError') return 'Nothing was shared. Try again and allow it, or pick another source.';
  if (name === 'NotFoundError' || name === 'OverconstrainedError') return 'That audio input is gone. Pick another.';
  return e instanceof Error ? e.message : String(e);
}

/** The beat heard now is a 1. */
export function liveDownbeat(): void {
  send({ type: 'liveDownbeat' });
}

/** The beat is twice (or half) as fast as the one found. */
export function liveTempo(double: boolean): void {
  send({ type: 'liveTempo', double });
}

/** Starts syncing: the next taps on the beat set the source's offset. */
export function startLiveSync(): void {
  syncTaps = [];
  set({ liveSyncTaps: 0, liveSyncError: null });
}

export function cancelLiveSync(): void {
  syncTaps = null;
  set({ liveSyncTaps: null });
}

function syncTap(ms: number, sixteenth: number): void {
  const offset = offsetToBeat(ms, sixteenth, get().bpm);
  if (!syncTaps || offset === null) return;
  syncTaps.push(offset);
  set({ liveSyncTaps: syncTaps.length });
  if (syncTaps.length < SYNC_TAPS) return;
  const { liveSource, liveOffsets } = get();
  const key = liveSource.status === 'none' ? null : liveSource.key;
  const next = key === null ? null : syncedOffset(liveOffsets[key] ?? 0, syncTaps);
  syncTaps = null;
  if (key === null || next === null) {
    set({ liveSyncTaps: null, liveSyncError: 'The taps were too uneven to go by. Try again, tapping right on the beat.' });
    return;
  }
  set({ liveSyncTaps: null, liveOffsets: { ...liveOffsets, [key]: next } });
  savePrefs();
  send({ type: 'setLiveOffset', ms: next });
  // The sync taps aren't drumming: no report on them.
  send({ type: 'stop' });
  skipJudged = true;
  judged = [];
  set({ judgedCount: 0 });
}

// ---- Recording ----------------------------------------------------------------

/** Downloads the last performance as a MIDI file. */
export function saveTake(): void {
  const { bpm, song } = get();
  const bytes = recorder?.export(bpm);
  if (!bytes) return;
  const stamp = new Date().toISOString().slice(0, 16).replace(/[:T]/g, '-');
  downloadMidi(bytes, `${fileName(song?.title ?? 'jam')} ${stamp}.mid`);
}

/** Downloads the parts transcribed from the open song's recording. */
export function saveTranscription(): void {
  const song = get().song;
  if (!song?.transcription) return;
  downloadMidi(transcriptionMidi(song.transcription, song.bpm, song.bars), `${fileName(song.title)} (transcribed).mid`);
}

const fileName = (title: string) => title.replace(/[^\w\- ]+/g, '').trim() || 'midzer';

function downloadMidi(bytes: Uint8Array, name: string): void {
  const url = URL.createObjectURL(new Blob([bytes as Uint8Array<ArrayBuffer>], { type: 'audio/midi' }));
  const a = document.createElement('a');
  a.href = url;
  a.download = name;
  a.click();
  setTimeout(() => URL.revokeObjectURL(url), 1000);
}

// ---- Player input -----------------------------------------------------------

/** Sends the player's notes to the engine and to whoever listens for them. */
function playerCommand(cmd: EngineCommand): void {
  send(cmd);
  notifyInput(cmd);
}

export function playerNoteOn(note: number, velocity = 100): void {
  playerCommand({ type: 'noteOn', note, velocity });
}

export function playerNoteOff(note: number): void {
  playerCommand({ type: 'noteOff', note });
}

function isTyping(target: EventTarget | null): boolean {
  return target instanceof HTMLElement && (target.isContentEditable || /^(INPUT|SELECT|TEXTAREA)$/.test(target.tagName));
}

/** Computer-keyboard playing. Returns a cleanup function. */
/** The computer keys are drum pads for drummers. */
const drumKeys = () => get().instrument === 'drums';

/** The key the space bar plays: the home note while soloing on level 1 over live audio. */
function spaceTap(): number | null {
  const { screen, soloLevel, soloKey } = get();
  return screen === 'along' && soloing() && soloLevel === 1 && soloKey ? homeKey(soloKey) : null;
}

export function attachQwerty(): () => void {
  const down = (e: KeyboardEvent) => {
    if (isTyping(e.target) || e.metaKey || e.ctrlKey || e.altKey) return;
    if (e.code === 'Space') {
      e.preventDefault();
      // Soloing on one key, the space bar is that key.
      const tap = spaceTap();
      if (tap !== null) {
        if (!e.repeat) playerNoteOn(tap);
      } else if (!e.repeat) togglePlay();
      return;
    }
    if (drumKeys()) {
      // Shift for an accent.
      const piece = qwertyToPiece(e.key);
      if (piece && !e.repeat) drumHit(piece, e.shiftKey ? 127 : 96);
      return;
    }
    const note = qwertyToNote(e.key);
    if (note !== undefined && !e.repeat) playerNoteOn(note);
  };
  const up = (e: KeyboardEvent) => {
    const tap = e.code === 'Space' ? spaceTap() : null;
    if (tap !== null) playerNoteOff(tap);
    if (drumKeys()) return;
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
  if (nativeMidi) {
    // Native inputs already reach the engine; the UI only hears about them.
    nativeMidi.onInput(notifyInput);
    nativeMidi.onPorts(({ inputs, outputs }) => {
      const { midiOutputId } = get();
      set({
        midiInputs: inputs,
        midiOutputs: outputs,
        midiOutputId: outputs.some((o) => o.id === midiOutputId) ? midiOutputId : null,
      });
    });
    return;
  }
  midiAccess = await requestMidi();
  if (!midiAccess) return;
  listenToInputs(midiAccess, playerCommand);
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
  nativeMidi?.setOutput(id).catch((e: unknown) => {
    console.warn('midzer: MIDI out', e);
    set({ midiOutputId: null });
  });
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
