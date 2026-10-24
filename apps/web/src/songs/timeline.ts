// The timeline editor's tools, as pure functions on a song's chart, beat map
// and hand fixes. Song beats count from the start of the song, one after
// another; a tool moves beats in time, regroups them into bars or moves
// chords between them. Sections, fills and lyrics stay on their beat.

import { BEATS_PER_BAR, barStarts, MAX_BAR_BEATS, resolvedBeats, totalBeats, type Bar, type Song, type SongFixes } from './song';

/** What the tools work on: the chart, when each of its beats is heard (plus the end of the last bar), and what was set by hand. */
export interface Timeline {
  bars: Bar[];
  beatTimes: number[];
  fixes: SongFixes;
}

/** The closest two beats may come, in seconds. */
const MIN_GAP = 0.05;

/** A song's timeline, if it has a beat map. */
export function timelineOf(song: Song): Timeline | null {
  const map = song.backing ?? song.heard;
  return map ? { bars: song.bars, beatTimes: map.beatTimes, fixes: song.fixes ?? {} } : null;
}

/** The song with the timeline's chart, beat map and fixes. */
export function withTimeline(song: Song, t: Timeline): Song {
  const next: Song = { ...song, bars: t.bars };
  if (song.backing) next.backing = { ...song.backing, beatTimes: t.beatTimes };
  else if (song.heard) next.heard = { ...song.heard, beatTimes: t.beatTimes };
  const fixes = cleanFixes(t.fixes);
  if (fixes) next.fixes = fixes;
  else delete next.fixes;
  return next;
}

function cleanFixes(f: SongFixes): SongFixes | undefined {
  const out: SongFixes = {};
  if (f.beats) out.beats = true;
  if (f.bars?.length) out.bars = [...new Set(f.bars)].sort((a, b) => a - b);
  if (f.chords?.length) out.chords = [...new Set(f.chords)].sort((a, b) => a - b);
  return Object.keys(out).length ? out : undefined;
}

/** A bar's section, fill and lyric. */
type Marks = Omit<Bar, 'beats'>;

/** The chart as one chord token a beat, its bar lines (song beats, from 0) and each bar's marks on its first beat. */
interface Flat {
  tokens: string[];
  lines: number[];
  marks: Map<number, Marks>;
}

function flatten(bars: Bar[]): Flat {
  const tokens: string[] = [];
  const lines: number[] = [];
  const marks = new Map<number, Marks>();
  for (const { beats, ...rest } of bars) {
    lines.push(tokens.length);
    if (Object.keys(rest).length) marks.set(tokens.length, rest);
    tokens.push(...beats);
  }
  return { tokens, lines, marks };
}

/** Bars from bar lines; a gap longer than a bar can be is split into fours. Each mark goes to the bar its beat is in. */
function build({ tokens, lines, marks }: Flat): Bar[] {
  const starts = [...new Set([0, ...lines])].filter((l) => l >= 0 && l < tokens.length).sort((a, b) => a - b);
  const bars: Bar[] = [];
  const at: number[] = [];
  starts.forEach((start, i) => {
    const end = starts[i + 1] ?? tokens.length;
    for (let s = start; s < end; ) {
      const n = end - s > MAX_BAR_BEATS ? BEATS_PER_BAR : end - s;
      bars.push({ beats: tokens.slice(s, s + n) });
      at.push(s);
      s += n;
    }
  });
  for (const [beat, m] of [...marks].sort((a, b) => a[0] - b[0])) {
    let i = at.length - 1;
    while (i > 0 && at[i]! > beat) i--;
    const bar = bars[i];
    if (!bar) continue;
    if (m.section && !bar.section) bar.section = m.section;
    if (m.fill) bar.fill = true;
    if (m.lyric) bar.lyric = bar.lyric ? `${bar.lyric} ${m.lyric}` : m.lyric;
  }
  return bars;
}

/** The beat map fitted to the chart: one time a beat plus the end, beats past the map going on at its last pace. */
function fitted(t: Timeline): number[] {
  const n = totalBeats(t.bars) + 1;
  const times = t.beatTimes.slice(0, n);
  const step = times.length >= 2 ? times[times.length - 1]! - times[times.length - 2]! : 0.5;
  while (times.length < n) times.push((times[times.length - 1] ?? 0) + step);
  return times;
}

/** The last beat heard at or before `seconds` (−1 before the first). */
function beatAtTime(times: number[], seconds: number): number {
  let lo = 0;
  let hi = times.length;
  while (lo < hi) {
    const mid = (lo + hi) >> 1;
    if (times[mid]! <= seconds) lo = mid + 1;
    else hi = mid;
  }
  return lo - 1;
}

/** The beat nearest in time to `seconds`, among the first `count`. */
export function nearestBeat(times: number[], seconds: number, count = times.length): number {
  const j = Math.min(count - 1, Math.max(0, beatAtTime(times, seconds)));
  const k = Math.min(count - 1, j + 1);
  return Math.abs(times[k]! - seconds) < Math.abs(times[j]! - seconds) ? k : j;
}

/** Chord tokens from the chord heard on each beat: a token where it changes. */
function tokensOf(heard: (string | null)[]): string[] {
  let before: string | null = null;
  return heard.map((chord) => {
    const token = chord === before ? '' : (chord ?? 'N.C.');
    before = chord;
    return token;
  });
}

function addFixes(list: number[] | undefined, beats: Iterable<number>): number[] {
  return [...new Set([...(list ?? []), ...beats])].sort((a, b) => a - b);
}

function range(from: number, to: number): number[] {
  return Array.from({ length: Math.max(0, to - from) }, (_, i) => from + i);
}

// ---- Bar lines ----------------------------------------------------------------

/** How long the bar `beat` is in would become if `beat` were a 1 (0 if it is one already). */
export function barCutAt(bars: Bar[], beat: number): number {
  const starts = barStarts(bars);
  for (let i = starts.length - 2; i >= 0; i--) if (starts[i]! <= beat) return beat - starts[i]!;
  return 0;
}

/**
 * "The 1 is here": `beat` becomes a bar line. The bar it was in ends there,
 * shorter; the bars after it are laid again in their lengths from the new
 * line, up to the next bar line set by hand (the last one cut short there)
 * or the end.
 */
export function oneIsHere(t: Timeline, beat: number): Timeline {
  const flat = flatten(t.bars);
  const total = flat.tokens.length;
  if (beat <= 0 || beat >= total) return t;
  const fixes = { ...t.fixes, bars: addFixes(t.fixes.bars, [beat]) };
  if (flat.lines.includes(beat)) return { ...t, fixes };
  const k = flat.lines.filter((l) => l < beat).length - 1;
  const stop = (t.fixes.bars ?? []).filter((l) => l > beat && flat.lines.includes(l)).sort((a, b) => a - b)[0] ?? total;
  const lengths: number[] = [];
  for (let j = k; j < flat.lines.length && flat.lines[j]! < stop; j++) lengths.push((flat.lines[j + 1] ?? total) - flat.lines[j]!);
  const lines = flat.lines.filter((l) => l <= flat.lines[k]! || l >= stop);
  let at = beat;
  for (const n of lengths) {
    if (at >= stop) break;
    lines.push(at);
    at += n;
  }
  return { ...t, bars: build({ ...flat, lines }), fixes };
}

/** "Not a 1": the bar line at `beat` goes, its bar joining the one before, if the two fit in one bar. */
export function notAOne(t: Timeline, beat: number): Timeline {
  const flat = flatten(t.bars);
  const i = flat.lines.indexOf(beat);
  if (i <= 0) return t;
  const end = flat.lines[i + 1] ?? flat.tokens.length;
  if (end - flat.lines[i - 1]! > MAX_BAR_BEATS) return t;
  const lines = flat.lines.filter((l) => l !== beat);
  return { ...t, bars: build({ ...flat, lines }), fixes: { ...t.fixes, bars: (t.fixes.bars ?? []).filter((l) => l !== beat) } };
}

/** Whether "Not a 1" can take the bar line at `beat`. */
export function canJoin(bars: Bar[], beat: number): boolean {
  const starts = barStarts(bars);
  const i = starts.indexOf(beat);
  return i > 0 && i < starts.length - 1 && starts[i + 1]! - starts[i - 1]! <= MAX_BAR_BEATS;
}

// ---- Beat times ---------------------------------------------------------------

/** Moves beats `first..=last` by `seconds`, as far as the beats on either side let them. */
export function nudge(t: Timeline, first: number, last: number, seconds: number): Timeline {
  const times = fitted(t);
  if (first < 0 || last >= times.length || first > last) return t;
  const low = first > 0 ? times[first - 1]! + MIN_GAP : 0;
  const high = last + 1 < times.length ? times[last + 1]! - MIN_GAP : Infinity;
  const by = Math.max(low - times[first]!, Math.min(high - times[last]!, seconds));
  if (by === 0) return t;
  for (let i = first; i <= last; i++) times[i] = times[i]! + by;
  return { ...t, beatTimes: times, fixes: { ...t.fixes, beats: true } };
}

/** Beats `first..=last` spaced evenly between the first and the last. */
export function evenOut(t: Timeline, first: number, last: number): Timeline {
  const times = fitted(t);
  if (first < 0 || last >= times.length || last - first < 2) return t;
  const a = times[first]!;
  const step = (times[last]! - a) / (last - first);
  for (let i = first + 1; i < last; i++) times[i] = a + (i - first) * step;
  return { ...t, beatTimes: times, fixes: { ...t.fixes, beats: true } };
}

/** The moment of a rise in the level near `seconds` (within `within`), if there is a clear one. */
export function onsetNear(envelope: Uint8Array, seconds: number, within = 0.06): number | null {
  const from = Math.max(2, Math.floor((seconds - within) * 100));
  const to = Math.min(envelope.length - 1, Math.ceil((seconds + within) * 100));
  let best = -1;
  let rise = 12; // about 3 dB in 20 ms
  for (let i = from; i <= to; i++) {
    const r = envelope[i]! - envelope[i - 2]!;
    if (r > rise) {
      rise = r;
      best = i;
    }
  }
  return best < 0 ? null : best / 100;
}

/**
 * Twice or half as many beats in `first..=last` (the whole song by default):
 * one between each pair, or every other one gone. Each chord, section, fill
 * and lyric stays where it was in time; bar lines are laid again in fours
 * from the first one in the range.
 */
export function changeTempo(t: Timeline, factor: 2 | 0.5, first = 0, last = Infinity): Timeline {
  const old = fitted(t);
  const flat = flatten(t.bars);
  const end = Math.min(last, old.length - 1);
  if (first < 0 || end - first < (factor === 2 ? 1 : 2)) return t;

  const inRange: number[] = [];
  if (factor === 2) {
    for (let i = first; i < end; i++) inRange.push(old[i]!, (old[i]! + old[i + 1]!) / 2);
    inRange.push(old[end]!);
  } else {
    for (let i = first; i <= end; i += 2) inRange.push(old[i]!);
    if ((end - first) % 2) inRange.push(old[end]!);
  }
  const times = [...old.slice(0, first), ...inRange, ...old.slice(end + 1)];
  const shift = first + inRange.length - 1 - end; // how far the beats after the range move
  const toNew = (beat: number) => (beat < first ? beat : beat >= end ? beat + shift : nearestBeat(times, old[beat]!, times.length - 1));
  const count = times.length - 1;

  // Chords by time: each new beat takes the chord heard as it starts.
  const heard = resolvedBeats(t.bars);
  const tokens = tokensOf(times.slice(0, count).map((s) => heard[beatAtTime(old, s + 0.001)] ?? null));

  const fromLine = flat.lines.find((l) => l >= first);
  const lines = flat.lines.filter((l) => fromLine === undefined || l < fromLine || l >= end).map(toNew);
  if (fromLine !== undefined && fromLine < end) for (let b = toNew(fromLine); b < toNew(end); b += BEATS_PER_BAR) lines.push(b);
  const marks = new Map<number, Marks>();
  for (const [beat, m] of flat.marks) marks.set(toNew(beat), m);

  const fixes: SongFixes = {
    beats: true,
    bars: (t.fixes.bars ?? []).filter((l) => l < first || l >= end).map(toNew),
    chords: [...new Set((t.fixes.chords ?? []).map(toNew))],
  };
  return { bars: build({ tokens, lines, marks }), beatTimes: times, fixes };
}

/**
 * Beats `first..=last` from the player's taps (seconds into the recording):
 * the tap nearest the range's first beat starts it, and each tap after is the
 * next beat, or the one after when a tap was missed. Beats between taps are
 * spaced evenly; those before the first tap or after the last move with it.
 */
export function fromTaps(t: Timeline, first: number, last: number, taps: number[]): Timeline {
  const times = fitted(t);
  if (first < 0 || last >= times.length || last <= first) return t;
  const sorted = [...taps].sort((a, b) => a - b);
  if (sorted.length < 2) return t;
  const gaps = sorted.slice(1).map((s, i) => s - sorted[i]!).sort((a, b) => a - b);
  const period = gaps[gaps.length >> 1]!;
  if (!(period > MIN_GAP)) return t;

  const set = new Map<number, number>();
  let at = first + nearestBeat(times.slice(first, last + 1), sorted[0]!);
  set.set(at, sorted[0]!);
  for (let k = 1; k < sorted.length; k++) {
    at += Math.max(1, Math.round((sorted[k]! - sorted[k - 1]!) / period));
    if (at > last) break;
    set.set(at, sorted[k]!);
  }
  const known = [...set.keys()];
  if (known.length < 2) return t;
  const a = known[0]!;
  const z = known[known.length - 1]!;
  const next = [...times];
  for (let i = 0; i + 1 < known.length; i++) {
    const [p, q] = [known[i]!, known[i + 1]!];
    for (let b = p; b <= q; b++) next[b] = set.get(p)! + ((set.get(q)! - set.get(p)!) * (b - p)) / (q - p);
  }
  for (let b = first; b < a; b++) next[b] = times[b]! + set.get(a)! - times[a]!;
  for (let b = z + 1; b <= last; b++) next[b] = times[b]! + set.get(z)! - times[z]!;
  if (next.some((s, i) => i > 0 && s - next[i - 1]! < MIN_GAP / 2) || next[0]! < 0) return t;
  return { ...t, beatTimes: next, fixes: { ...t.fixes, beats: true } };
}

// ---- Chords -------------------------------------------------------------------

/** A chord held over beats `start..end`; null is no chord. */
export interface ChordBlock {
  start: number;
  end: number;
  name: string | null;
}

/** The chart's chords as blocks, one per chord held. */
export function chordBlocks(bars: Bar[]): ChordBlock[] {
  const blocks: ChordBlock[] = [];
  resolvedBeats(bars).forEach((name, beat) => {
    const last = blocks[blocks.length - 1];
    if (last && last.name === name) last.end = beat + 1;
    else blocks.push({ start: beat, end: beat + 1, name });
  });
  return blocks;
}

/** The chord from `block.start` on becomes `name` (null: no chord), for the whole block. */
export function setChord(t: Timeline, block: ChordBlock, name: string | null): Timeline {
  const flat = flatten(t.bars);
  if (block.start < 0 || block.start >= flat.tokens.length) return t;
  flat.tokens[block.start] = name ?? 'N.C.';
  for (let b = block.start + 1; b < block.end; b++) flat.tokens[b] = '';
  return { ...t, bars: build(flat), fixes: { ...t.fixes, chords: addFixes(t.fixes.chords, range(block.start, block.end)) } };
}

/** Moves the change of chord at beat `edge` to beat `to`, between the changes on either side. */
export function moveChordEdge(t: Timeline, edge: number, to: number): Timeline {
  const blocks = chordBlocks(t.bars);
  const i = blocks.findIndex((b) => b.start === edge);
  if (i <= 0) return t;
  const target = Math.max(blocks[i - 1]!.start + 1, Math.min(blocks[i]!.end - 1, to));
  if (target === edge) return t;
  const flat = flatten(t.bars);
  const right = blocks[i]!.name;
  for (let b = Math.min(edge, target); b <= Math.max(edge, target); b++) flat.tokens[b] = '';
  flat.tokens[target] = right ?? 'N.C.';
  return { ...t, bars: build(flat), fixes: { ...t.fixes, chords: addFixes(t.fixes.chords, range(Math.min(edge, target), Math.max(edge, target) + 1)) } };
}

/** The song's chords, the most used first, to pick from. */
export function songChords(bars: Bar[], limit = 8): string[] {
  const count = new Map<string, number>();
  for (const name of resolvedBeats(bars)) if (name) count.set(name, (count.get(name) ?? 0) + 1);
  return [...count].sort((a, b) => b[1] - a[1]).slice(0, limit).map(([name]) => name);
}

// ---- Fixes from the chart grid, and analysing again -----------------------------------

/** The fixes after a hand edit of the chart from `before` to `after`: beats whose chord changed, bar lines that are new. */
export function chartFixes(before: Bar[], after: Bar[], fixes: SongFixes = {}): SongFixes {
  const a = resolvedBeats(before);
  const b = resolvedBeats(after);
  const changed = b.flatMap((name, i) => (name !== a[i] ? [i] : []));
  const old = new Set(barStarts(before).slice(0, -1));
  const lines = barStarts(after).slice(1, -1).filter((l) => !old.has(l));
  const next: SongFixes = { ...fixes };
  if (changed.length) next.chords = addFixes(fixes.chords, changed);
  if (lines.length) next.bars = addFixes(fixes.bars, lines);
  return next;
}

/**
 * "Keep my fixes" when analysing again: what was set by hand stays, the rest
 * comes from `fresh`. With the beats fixed, the old beat map and bars stay and
 * the new chords move onto them by time (each beat takes the chord over its
 * middle). Otherwise the new beat map and bars come in, and each bar line set
 * by hand is set again ("The 1 is here") at the new beat nearest it. Chords set by hand stay, moved by
 * time if the beats changed. Sections come from `fresh`; fills and lyrics stay
 * where they were in time.
 */
export function keepFixes(old: Timeline, fresh: { bars: Bar[]; beatTimes: number[] }): Timeline {
  const oldTimes = fitted(old);
  const oldHeard = resolvedBeats(old.bars);
  const oldFlat = flatten(old.bars);
  const freshTimes = fitted({ ...fresh, fixes: {} });
  const freshHeard = resolvedBeats(fresh.bars);
  const freshFlat = flatten(fresh.bars);
  const fixes = old.fixes;

  const times = fixes.beats ? oldTimes : freshTimes;
  const count = times.length - 1;
  const toNew = (beat: number) => (fixes.beats ? beat : nearestBeat(times, oldTimes[beat]!, count));

  // Chords: the new ones by time on the old beats, or as they come; then the hand-set ones.
  const heard = fixes.beats
    ? range(0, count).map((b) => freshHeard[beatAtTime(freshTimes, (times[b]! + times[b + 1]!) / 2)] ?? null)
    : freshHeard.slice(0, count);
  for (const b of fixes.chords ?? []) if (b < oldHeard.length) heard[toNew(b)] = oldHeard[b] ?? null;

  const handLines = [...new Set((fixes.bars ?? []).filter((l) => l < oldTimes.length - 1).map(toNew))];

  const marks = new Map<number, Marks>();
  const freshStarts = barStarts(fresh.bars);
  fresh.bars.forEach((bar, i) => {
    if (!bar.section) return;
    const at = fixes.beats ? nearestBeat(times, freshTimes[freshStarts[i]!]!, count) : freshStarts[i]!;
    marks.set(at, { section: bar.section });
  });
  for (const [beat, { fill, lyric }] of oldFlat.marks) {
    if (!fill && !lyric) continue;
    const at = toNew(beat);
    marks.set(at, { ...marks.get(at), ...(fill ? { fill } : {}), ...(lyric ? { lyric } : {}) });
  }

  let kept: Timeline = {
    bars: build({ tokens: tokensOf(heard), lines: fixes.beats ? oldFlat.lines : freshFlat.lines, marks }),
    beatTimes: times,
    fixes: { ...fixes, bars: fixes.beats ? handLines : [], chords: (fixes.chords ?? []).filter((b) => b < oldHeard.length).map(toNew) },
  };
  // On new beats, each line set by hand is set again, as by hand: the last first, so each one lays its bars up to the next.
  if (!fixes.beats) for (const line of handLines.sort((a, b) => b - a)) kept = oneIsHere(kept, line);
  return kept;
}
