// Learn while it plays: builds the targets for the open song, judges the
// player's notes on the song clock, moves the band's wait point in wait mode,
// lights the keys to play next and records finished runs.

import { NOTE_NAMES, PPQ, type TimedEvent } from '@midzer/audio-web';
import { create } from 'zustand';
import { live } from '../live';
import { closeSong, onEngineEvents, onPlayerNote, openSong, seekSong, send, setLoop, setTempo, useSession } from '../session';
import { barStarts, totalBeats, type Song } from '../songs/song';
import { SongClock } from './clock';
import { accuracy, emptyScore, Judge, starsFor, type Grade, type Score } from './judge';
import type { BeatRange } from './lanes';
import { lessonAfter, type Lesson } from './lessons';
import { loadProgress, recordRun, songProgressId, type Progress } from './progress';
import { buildTargets, targetsEnd, type Part, type Target } from './targets';

/** Wait mode lights the keys this many beats before the band gets there. */
const HINT_AHEAD_BEATS = 1;

export interface Feedback {
  grade: Grade;
  /** ms early (<0) or late (>0); null in wait mode. */
  offsetMs: number | null;
  /** Bumped on each judgement so the same grade twice still shows. */
  seq: number;
}

export interface RunResult {
  score: Score;
  stars: 0 | 1 | 2 | 3;
  accuracy: number;
  /** Stars before this run. */
  previousStars: number;
  next: Lesson | null;
}

interface PracticeState {
  lesson: Lesson | null;
  song: Song | null;
  part: Part;
  wait: boolean;
  /** Share of the song's tempo. */
  speed: number;
  anyOctave: boolean;
  targetCount: number;
  score: Score;
  /** What to play now, and after that. */
  now: Target[];
  next: Target[];
  holding: boolean;
  feedback: Feedback | null;
  result: RunResult | null;
  progress: Map<string, Progress>;
}

export const usePractice = create<PracticeState>(() => ({
  lesson: null,
  song: null,
  part: 'chords',
  wait: true,
  speed: 1,
  anyOctave: true,
  targetCount: 0,
  score: emptyScore(),
  now: [],
  next: [],
  holding: false,
  feedback: null,
  result: null,
  progress: new Map(),
}));

const set = usePractice.setState;
const get = usePractice.getState;
const session = useSession.getState;

/** Per-frame state for the falling notes, outside React. */
export const practiceLive = {
  clock: new SongClock(),
  judge: null as Judge | null,
  targets: [] as Target[],
  loop: null as BeatRange | null,
  /** Where the song starts, in beats, for the view while stopped. */
  startBeat: 0,
  /** The beat each bar starts on, and the song's end. */
  barStarts: [0] as number[],
  /** Target id → time it was played, for a flash. */
  flashes: new Map<number, number>(),
  lastPos: null as number | null,
};

let sentWait: number | null | undefined;
let feedbackSeq = 0;

/** A name for what a target asks for: a chord's name or a note's, e.g. "Am" or "E4". */
export function targetName(t: Target): string {
  if (t.kind === 'chord') return t.name ?? '?';
  const key = t.keys[0]!;
  return `${NOTE_NAMES[key % 12]}${Math.floor(key / 12) - 1}`;
}

export function progressId(state: Pick<PracticeState, 'lesson' | 'song' | 'part'>): string {
  return state.lesson?.id ?? songProgressId(state.song?.id ?? '', state.part);
}

export async function refreshProgress(): Promise<void> {
  set({ progress: await loadProgress() });
}

/** The computer keyboard has one octave, so melodies count in any octave without a MIDI device. */
const hasMidiInput = () => session().midiInputs.length > 0;

export function openLesson(lesson: Lesson): void {
  open(lesson.song, lesson.part, lesson, { wait: lesson.wait, speed: lesson.speed });
}

/** Practises any song outside the lesson path. */
export function openPractice(song: Song, part: Part): void {
  open(song, part, null, { wait: true, speed: 1 });
}

function open(song: Song, part: Part, lesson: Lesson | null, { wait, speed }: { wait: boolean; speed: number }): void {
  openSong(song, [], { screen: 'lesson', speed });
  set({ lesson, song, part, wait, speed, anyOctave: !hasMidiInput(), result: null, feedback: null, holding: false });
  practiceLive.loop = null;
  rebuild();
}

export function closePractice(): void {
  closeSong('learn');
  set({ song: null, lesson: null, result: null });
  clearHints();
}

/** New targets and a fresh judge for the next run; stops the band if it plays, unless `starting`. */
function rebuild(starting = false): void {
  const { song, part, wait, anyOctave } = get();
  if (!song) return;
  if (session().playing && !starting) send({ type: 'stop' });
  const targets = buildTargets(song, part, { chordBase: hasMidiInput() ? 48 : 60 });
  practiceLive.targets = targets;
  practiceLive.barStarts = barStarts(song.bars);
  practiceLive.clock.reset();
  practiceLive.flashes.clear();
  practiceLive.lastPos = null;
  practiceLive.startBeat = practiceLive.loop?.start ?? 0;
  const judge = new Judge(targets, { wait, anyOctave });
  judge.skipBefore(practiceLive.startBeat);
  practiceLive.judge = judge;
  set({ targetCount: targets.length, score: { ...judge.score }, feedback: null });
  publishTargets();
  sentWait = undefined;
  updateWait();
}

export function setPart(part: Part): void {
  set({ part });
  rebuild();
}

export function setWait(wait: boolean): void {
  set({ wait });
  rebuild();
}

export function setAnyOctave(anyOctave: boolean): void {
  set({ anyOctave });
  rebuild();
}

export function setSpeed(speed: number): void {
  const song = get().song;
  set({ speed });
  if (song) setTempo(song.bpm * speed);
}

/** Loops bars `start..end` (and starts there), or plays the whole song. */
export function setPracticeLoop(bars: { start: number; end: number } | null): void {
  setLoop(bars);
  seekSong(bars?.start ?? 0);
  const starts = barStarts(get().song?.bars ?? []);
  practiceLive.loop = bars && { start: starts[bars.start] ?? 0, end: starts[bars.end] ?? starts[starts.length - 1]! };
  rebuild();
}

export function dismissResult(): void {
  set({ result: null });
}

/** Subscribes to the engine and the player and runs the per-frame work. Returns a cleanup function. */
export function attachPractice(): () => void {
  const offEngine = onEngineEvents(onEngine);
  const offPlayer = onPlayerNote(onNote);
  let frame = requestAnimationFrame(function tick() {
    onFrame(performance.now());
    frame = requestAnimationFrame(tick);
  });
  return () => {
    offEngine();
    offPlayer();
    cancelAnimationFrame(frame);
    clearHints();
  };
}

function onEngine(event: TimedEvent, timeMs: number): void {
  const clock = practiceLive.clock;
  const delay = Math.max(0, timeMs - performance.now());
  switch (event.type) {
    case 'songPosition':
      clock.onBeat(event.beat, timeMs);
      break;
    case 'hold':
      clock.onHold(event.holding, event.tick / PPQ, timeMs);
      setTimeout(() => set({ holding: event.holding }), delay);
      break;
    case 'transport':
      if (event.playing) startRun();
      else setTimeout(endRun, delay);
      break;
  }
}

/** A new performance starts from a fresh judge and clock. */
function startRun(): void {
  set({ result: null });
  rebuild(true);
}

/** The band stopped. A run that reached the end of the song is graded and saved. */
function endRun(): void {
  set({ holding: false });
  const judge = practiceLive.judge;
  const pos = practiceLive.lastPos;
  const { song } = get();
  if (!judge || !song || pos === null) return;
  const ended = !practiceLive.loop && pos >= Math.max(targetsEnd(practiceLive.targets), totalBeats(song.bars)) - 1.5;
  if (!ended) return;
  judge.advance(Number.MAX_SAFE_INTEGER, practiceLive.clock.msPerBeat);
  const score = { ...judge.score };
  set({ score });
  const state = get();
  const id = progressId(state);
  const previousStars = state.progress.get(id)?.stars ?? 0;
  const result: RunResult = {
    score,
    stars: starsFor(score),
    accuracy: accuracy(score),
    previousStars,
    next: state.lesson ? lessonAfter(state.lesson.id) : null,
  };
  set({ result });
  void recordRun(id, score).then((p) => set({ progress: new Map(get().progress).set(id, p) }));
}

function onNote(key: number, on: boolean, timeMs: number): void {
  const judge = practiceLive.judge;
  if (!judge) return;
  if (!on) {
    judge.noteOff(key);
    return;
  }
  const pos = practiceLive.clock.position(timeMs - (session().tapOffsetMs ?? 0));
  if (pos === null || !session().playing) return;
  const judged = judge.noteOn(key, pos, practiceLive.clock.msPerBeat);
  for (const j of judged) practiceLive.flashes.set(j.target.id, performance.now());
  if (judged.length > 0) {
    const last = judged[judged.length - 1]!;
    set({ feedback: { grade: last.grade, offsetMs: last.offsetMs, seq: ++feedbackSeq } });
    publishTargets();
    updateWait();
  }
  set({ score: { ...judge.score } });
}

function onFrame(now: number): void {
  const judge = practiceLive.judge;
  if (!judge) return;
  practiceLive.clock.setTempo(session().bpm);
  const pos = practiceLive.clock.position(now);
  if (pos !== null && session().playing) {
    practiceLive.lastPos = pos;
    const missed = judge.advance(pos, practiceLive.clock.msPerBeat);
    if (missed.length > 0) {
      set({ score: { ...judge.score }, feedback: { grade: 'miss', offsetMs: null, seq: ++feedbackSeq } });
    }
    publishTargets();
    updateWait();
  }
  updateHints(pos ?? practiceLive.startBeat);
}

/** Wait mode: the band stops at the first target not played yet. */
function updateWait(): void {
  const { wait } = get();
  const next = wait ? practiceLive.judge?.nextOpen() : null;
  const tick = next ? Math.round(next.beat * PPQ) : null;
  if (tick !== sentWait) {
    sentWait = tick;
    send({ type: 'songWait', tick });
  }
}

function publishTargets(): void {
  const judge = practiceLive.judge;
  if (!judge) return;
  const now = judge.current();
  const first = now[0];
  const next = first
    ? practiceLive.targets.filter((t, i) => judge.states[i] === 'open' && t.beat > first.beat + 1e-6)
    : [];
  const after = next.filter((t) => Math.abs(t.beat - next[0]!.beat) < 1e-6);
  const { now: shownNow, next: shownNext } = get();
  if (!sameTargets(shownNow, now) || !sameTargets(shownNext, after)) set({ now, next: after });
}

const sameTargets = (a: Target[], b: Target[]) => a.length === b.length && a.every((t, i) => t.id === b[i]!.id);

let hinted = '';

/** Lights the keys of the targets coming up next. */
function updateHints(pos: number): void {
  const now = get().now;
  const due = now.filter((t) => t.beat - pos <= HINT_AHEAD_BEATS);
  const key = due.map((t) => t.id).join(',');
  if (key === hinted) return;
  hinted = key;
  live.hint.fill(0);
  for (const t of due) for (const k of t.keys) live.hint[k] = t.kind === 'chord' ? 2 : 1;
  live.version++;
}

function clearHints(): void {
  hinted = '';
  live.hint.fill(0);
  live.version++;
}
