import { useEffect, useMemo, useRef, useState, type KeyboardEvent, type PointerEvent } from 'react';
import { live } from '../live';
import {
  editSong,
  onDrumInput,
  send,
  onPlayerNote,
  seekSong,
  setPlayRecording,
  songEnvelope,
  useSession,
} from '../session';
import { isNoChord, parseChord } from '../songs/chords';
import { ENVELOPE_FPS } from '../songs/envelope';
import { barAt, barStarts, type Song } from '../songs/song';
import {
  barCutAt,
  canJoin,
  changeTempo,
  chordBlocks,
  evenOut,
  fromTaps,
  moveChordEdge,
  nearestBeat,
  notAOne,
  nudge,
  oneIsHere,
  onsetNear,
  setChord,
  songChords,
  timelineOf,
  withTimeline,
  type ChordBlock,
  type Timeline,
} from '../songs/timeline';
import { color, font } from '../theme';

const HEIGHT = 168;
/** Bar numbers above, the level in the middle, chord blocks below. */
const LEVEL_TOP = 20;
const LEVEL_BOTTOM = 120;
const CHORD_TOP = 128;
const CHORD_BOTTOM = 162;
const DEFAULT_PPS = 80;
const MIN_PPS = 10;
const MAX_PPS = 800;
/** How close, in px, a pointer must come to a line to drag it. */
const GRAB_PX = 6;
/** Dragging a single beat snaps to a rise in the level this close, in seconds. */
const SNAP_S = 0.03;

interface BeatRange {
  first: number;
  last: number;
}

type Drag =
  | { kind: 'beats'; range: BeatRange; beat: number; x0: number; moved: boolean }
  | { kind: 'edge'; edge: number; x0: number; moved: boolean }
  | { kind: 'chord'; block: ChordBlock }
  | { kind: 'click'; beat: number; shift: boolean };

/**
 * The song's beat map on a scrolled, zoomed canvas, for fixing it by hand:
 * the level heard, beat lines with the bar lines brighter, the chords as
 * blocks, and the playhead while the recording plays. Its tools act on the
 * beats selected; every change is an edit of the song (undone, saved and
 * analysed again like chart edits).
 */
export function TimelineEditor({ song }: { song: Song }) {
  const timeline = useMemo(() => timelineOf(song), [song]);
  const [preview, setPreview] = useState<Timeline | null>(null);
  const [pps, setPps] = useState(DEFAULT_PPS);
  const [sel, setSel] = useState<BeatRange | null>(null);
  const [anchor, setAnchor] = useState(0);
  const [envelope, setEnvelope] = useState<Uint8Array | null>(null);
  const [chordEdit, setChordEdit] = useState<ChordBlock | null>(null);
  const [askOne, setAskOne] = useState<number | null>(null);
  const [tapping, setTapping] = useState<BeatRange | null>(null);
  const [note, setNote] = useState<string | null>(null);
  const playing = useSession((s) => s.playing);
  const recordingReady = useSession((s) => s.recording.status === 'ready');
  const scroller = useRef<HTMLDivElement>(null);
  const canvas = useRef<HTMLCanvasElement>(null);
  const drag = useRef<Drag | null>(null);

  const shown = preview ?? timeline;
  const starts = useMemo(() => barStarts(shown?.bars ?? []), [shown?.bars]);
  const barOf = useMemo(() => new Map(starts.slice(0, -1).map((beat, bar) => [beat, bar])), [starts]);
  const blocks = useMemo(() => chordBlocks(shown?.bars ?? []), [shown?.bars]);
  const view = useRef<View>({ shown, starts, barOf, blocks, pps, sel, envelope, chordEdit, playing });
  view.current = { shown, starts, barOf, blocks, pps, sel, envelope, chordEdit, playing };

  useEffect(() => {
    let on = true;
    void songEnvelope().then((e) => on && setEnvelope(e));
    return () => {
      on = false;
    };
  }, [song.id]);

  // Drawn every frame from `view`, so the playhead moves and scrolling needs no re-render.
  useEffect(() => {
    let frame = 0;
    const loop = () => {
      if (canvas.current && scroller.current) draw(canvas.current, scroller.current, view.current);
      frame = requestAnimationFrame(loop);
    };
    frame = requestAnimationFrame(loop);
    return () => cancelAnimationFrame(frame);
  }, []);

  // Ctrl-wheel (and a trackpad pinch) zooms around the pointer.
  useEffect(() => {
    const el = scroller.current!;
    const wheel = (e: WheelEvent) => {
      if (!e.ctrlKey) return;
      e.preventDefault();
      const x = e.clientX - el.getBoundingClientRect().left;
      const old = view.current.pps;
      const next = Math.min(MAX_PPS, Math.max(MIN_PPS, old * Math.exp(-e.deltaY * 0.002)));
      const at = (el.scrollLeft + x) / old;
      setPps(next);
      requestAnimationFrame(() => (el.scrollLeft = at * next - x));
    };
    el.addEventListener('wheel', wheel, { passive: false });
    return () => el.removeEventListener('wheel', wheel);
  }, []);

  if (!timeline || !shown) return null;
  const times = shown.beatTimes;
  const map = song.backing ?? song.heard;
  const seconds = Math.max(map?.seconds ?? 0, times[times.length - 1] ?? 0) + 1;
  const commit = (t: Timeline, kind?: string) => t !== timeline && editSong(withTimeline(song, t), kind);
  const beatCount = Math.min(times.length, starts[starts.length - 1]! + 1);
  const many = sel !== null && sel.last > sel.first;

  const xOf = (clientX: number) => clientX - canvas.current!.getBoundingClientRect().left + scroller.current!.scrollLeft;
  const beatAt = (x: number) => nearestBeat(times, x / pps, beatCount);
  const select = (beat: number, shift: boolean) => {
    if (shift && sel) setSel({ first: Math.min(anchor, beat), last: Math.max(anchor, beat) });
    else {
      setSel({ first: beat, last: beat });
      setAnchor(beat);
    }
    setAskOne(null);
    setNote(null);
  };

  const onPointerDown = (e: PointerEvent<HTMLCanvasElement>) => {
    if (tapping) return;
    const x = xOf(e.clientX);
    const y = e.clientY - canvas.current!.getBoundingClientRect().top;
    e.currentTarget.setPointerCapture(e.pointerId);
    if (y >= CHORD_TOP) {
      const t = x / pps;
      const i = blocks.findIndex((b) => (times[b.start] ?? Infinity) <= t && t < (times[b.end] ?? Infinity));
      const block = blocks[i];
      if (!block) return;
      const near = (beat: number) => Math.abs((times[beat] ?? Infinity) * pps - x) <= GRAB_PX;
      if (i > 0 && near(block.start)) drag.current = { kind: 'edge', edge: block.start, x0: x, moved: false };
      else if (near(block.end) && blocks[i + 1]) drag.current = { kind: 'edge', edge: block.end, x0: x, moved: false };
      else drag.current = { kind: 'chord', block };
      return;
    }
    const beat = beatAt(x);
    if (Math.abs(times[beat]! * pps - x) > GRAB_PX) {
      drag.current = { kind: 'click', beat, shift: e.shiftKey };
      return;
    }
    const inSel = sel && beat >= sel.first && beat <= sel.last;
    if (!inSel) select(beat, e.shiftKey);
    const range = inSel ? sel : e.shiftKey && sel ? { first: Math.min(anchor, beat), last: Math.max(anchor, beat) } : { first: beat, last: beat };
    drag.current = { kind: 'beats', range, beat, x0: x, moved: false };
  };

  const onPointerMove = (e: PointerEvent<HTMLCanvasElement>) => {
    const d = drag.current;
    if (!d || (d.kind !== 'beats' && d.kind !== 'edge')) return;
    const x = xOf(e.clientX);
    if (!d.moved && Math.abs(x - d.x0) < 3) return;
    d.moved = true;
    if (d.kind === 'edge') {
      setPreview(moveChordEdge(timeline, d.edge, nearestBeat(timeline.beatTimes, x / pps, beatCount)));
      return;
    }
    let by = (x - d.x0) / pps;
    const from = timeline.beatTimes[d.beat]!;
    if (envelope && d.range.first === d.range.last && !e.altKey) {
      const onset = onsetNear(envelope, from + by, SNAP_S);
      if (onset !== null) by = onset - from;
    }
    setPreview(nudge(timeline, d.range.first, d.range.last, by));
  };

  const onPointerUp = () => {
    const d = drag.current;
    drag.current = null;
    if (preview) {
      commit(preview);
      setPreview(null);
      return;
    }
    if (d?.kind === 'chord') {
      setChordEdit(d.block);
      setNote(null);
    } else if (d?.kind === 'click' || (d?.kind === 'beats' && !d.moved)) {
      const beat = d.beat;
      if (d.kind === 'click') select(beat, d.shift);
      else if (d.range.first !== d.range.last && sel?.first === d.range.first) select(beat, false);
      setChordEdit(null);
      if (!(d.kind === 'click' && d.shift)) seekSong(barAt(starts, beat).bar);
    }
  };

  const onKeyDown = (e: KeyboardEvent<HTMLDivElement>) => {
    if (!sel || (e.key !== 'ArrowLeft' && e.key !== 'ArrowRight')) return;
    e.preventDefault();
    const step = e.key === 'ArrowLeft' ? -1 : 1;
    if (e.altKey) {
      const beat = Math.min(beatCount - 1, Math.max(0, (step < 0 ? sel.first : sel.last) + step));
      select(beat, e.shiftKey);
      return;
    }
    commit(nudge(timeline, sel.first, sel.last, step * (e.shiftKey ? 0.001 : 0.01)), `nudge ${sel.first} ${sel.last}`);
  };

  const one = (confirmed = false) => {
    if (!sel) return;
    if (!confirmed && barCutAt(timeline.bars, sel.first) === 1) return setAskOne(sel.first);
    setAskOne(null);
    commit(oneIsHere(timeline, sel.first));
  };
  const tempo = (factor: 2 | 0.5) => {
    commit(many ? changeTempo(timeline, factor, sel.first, sel.last) : changeTempo(timeline, factor));
    setSel(null);
  };
  const startTapping = () => {
    if (!sel || !many) return;
    send({ type: 'stop' });
    setPlayRecording(true);
    seekSong(Math.max(0, barAt(starts, sel.first).bar - 1));
    setTapping(sel);
    setNote(null);
    send({ type: 'play' });
  };
  const finishTapping = (taps: number[]) => {
    const range = tapping;
    setTapping(null);
    if (!range) return;
    const next = fromTaps(timeline, range.first, range.last, taps);
    if (next === timeline) setNote('Too few taps to place the beats: tap each beat of the selection.');
    else commit(next);
  };

  const selText = !sel
    ? 'Click a beat line to select it'
    : sel.first === sel.last
      ? `Beat ${sel.first + 1} · bar ${barAt(starts, sel.first).bar + 1}, beat ${barAt(starts, sel.first).beat + 1}`
      : `Beats ${sel.first + 1}–${sel.last + 1}`;
  const isLine = sel !== null && starts.includes(sel.first);

  return (
    <div className="timeline-editor">
      <div className="edit-tools timeline-tools" role="toolbar" aria-label="Edit the beat">
        <span className="label" role="status">
          {selText}
        </span>
        <button className="btn btn-secondary" disabled={!sel || sel.first === 0 || isLine || !!tapping} onClick={() => one()}>
          The 1 is here
        </button>
        <button className="btn btn-secondary" disabled={!sel || !canJoin(timeline.bars, sel.first) || !!tapping} onClick={() => sel && commit(notAOne(timeline, sel.first))}>
          Not a 1
        </button>
        <button className="btn btn-secondary btn-square" aria-label="Earlier by 10 ms" disabled={!sel || !!tapping} onClick={() => sel && commit(nudge(timeline, sel.first, sel.last, -0.01), `nudge ${sel.first} ${sel.last}`)}>
          ←
        </button>
        <button className="btn btn-secondary btn-square" aria-label="Later by 10 ms" disabled={!sel || !!tapping} onClick={() => sel && commit(nudge(timeline, sel.first, sel.last, 0.01), `nudge ${sel.first} ${sel.last}`)}>
          →
        </button>
        <button className="btn btn-secondary" disabled={!sel || sel.last - sel.first < 2 || !!tapping} onClick={() => sel && commit(evenOut(timeline, sel.first, sel.last))}>
          Even out
        </button>
        <button className="btn btn-secondary" disabled={!!tapping} title={many ? 'Every other selected beat goes' : 'Every other beat of the song goes'} onClick={() => tempo(0.5)}>
          Half tempo
        </button>
        <button className="btn btn-secondary" disabled={!!tapping} title={many ? 'A beat between each pair selected' : 'A beat between each pair in the song'} onClick={() => tempo(2)}>
          Double tempo
        </button>
        {song.backing && (
          <button className="btn btn-secondary" disabled={!many || !recordingReady || !!tapping} title="Play from the bar before and tap each selected beat" onClick={startTapping}>
            Tap along
          </button>
        )}
        <span className="timeline-zoom">
          <button className="btn btn-ghost btn-square" aria-label="Zoom out" onClick={() => setPps(Math.max(MIN_PPS, pps / 1.5))}>
            −
          </button>
          <button className="btn btn-ghost btn-square" aria-label="Zoom in" onClick={() => setPps(Math.min(MAX_PPS, pps * 1.5))}>
            +
          </button>
        </span>
      </div>
      {askOne !== null && (
        <div className="timeline-ask" role="alert">
          <span>That leaves bar {barAt(starts, askOne).bar + 1} one beat long. Is that right?</span>
          <button className="btn btn-primary" onClick={() => one(true)}>
            Yes, a 1 here
          </button>
          <button className="btn btn-ghost" onClick={() => setAskOne(null)}>
            No
          </button>
        </div>
      )}
      {tapping && <Tapping range={tapping} times={timeline.beatTimes} onDone={finishTapping} />}
      <div
        ref={scroller}
        className="timeline-scroll"
        tabIndex={0}
        aria-label="Beat timeline: click a beat line to select it, drag it to move it; arrow keys move the selection by 10 ms (with shift 1 ms), alt and an arrow selects the next beat"
        onKeyDown={onKeyDown}
      >
        <div className="timeline-track" style={{ width: Math.ceil(seconds * pps) }}>
          <canvas ref={canvas} onPointerDown={onPointerDown} onPointerMove={onPointerMove} onPointerUp={onPointerUp} onPointerCancel={() => ((drag.current = null), setPreview(null))} />
        </div>
      </div>
      {chordEdit && <ChordPicker key={chordEdit.start} block={chordEdit} song={song} starts={starts} onPick={(name) => (commit(setChord(timeline, chordEdit, name)), setChordEdit(null))} onCancel={() => setChordEdit(null)} />}
      {note && (
        <p className="status-line error" role="alert">
          {note}
        </p>
      )}
      <p className="setting-note">
        Drag a beat line (it snaps to the beat heard; hold alt not to), or select beats and use the tools. Drag the edge of a chord to move the change, click it to change the chord. Ctrl-wheel zooms.
      </p>
    </div>
  );
}

/** While tapping along: each tap (space, a key or a pad) is placed in the recording, until the selection has played. */
function Tapping({ range, times, onDone }: { range: BeatRange; times: number[]; onDone: (taps: number[]) => void }) {
  const tapOffsetMs = useSession((s) => s.tapOffsetMs);
  const [count, setCount] = useState(0);
  const taps = useRef<number[]>([]);
  const done = useRef(onDone);
  done.current = onDone;

  useEffect(() => {
    const started = performance.now();
    const finish = () => {
      stop();
      send({ type: 'stop' });
      done.current(taps.current);
    };
    const tap = (ms: number) => {
      const clock = live.songClock;
      const at = times[clock.beat];
      // Only beats heard since tapping started place a tap.
      if (at === undefined || clock.ms < started || ms - clock.ms > 5000) return;
      // Where the tap meant to be: before the player's own lag.
      taps.current.push(at + (ms - clock.ms - (tapOffsetMs ?? 0)) / 1000);
      setCount(taps.current.length);
    };
    const key = (e: globalThis.KeyboardEvent) => {
      if (e.code !== 'Space' && e.key !== 'Escape') return;
      e.preventDefault();
      e.stopImmediatePropagation();
      if (e.key === 'Escape') finish();
      else if (!e.repeat) tap(performance.now());
    };
    window.addEventListener('keydown', key, { capture: true });
    const offNote = onPlayerNote((_, on, ms) => on && tap(ms));
    const offDrum = onDrumInput(() => tap(performance.now()));
    const timer = setInterval(() => {
      if (live.songClock.ms > started && live.songClock.beat > range.last + 1) finish();
    }, 100);
    function stop() {
      window.removeEventListener('keydown', key, { capture: true });
      offNote();
      offDrum();
      clearInterval(timer);
    }
    return stop;
  }, [range, times, tapOffsetMs]);

  return (
    <div className="timeline-ask" role="status">
      <span>
        Tap each beat from {range.first + 1} to {range.last + 1} (space, a key or a pad) · {count} {count === 1 ? 'tap' : 'taps'}
      </span>
      <button className="btn btn-primary" onClick={() => (send({ type: 'stop' }), onDone(taps.current))}>
        Done
      </button>
    </div>
  );
}

/** Types or picks the chord of a block. */
function ChordPicker({ block, song, starts, onPick, onCancel }: { block: ChordBlock; song: Song; starts: number[]; onPick: (name: string | null) => void; onCancel: () => void }) {
  const [text, setText] = useState(block.name ?? '');
  const [error, setError] = useState(false);
  const picks = useMemo(() => songChords(song.bars), [song.bars]);
  const at = barAt(starts, block.start);
  const apply = () => {
    const name = text.trim();
    if (!name || isNoChord(name)) return onPick(null);
    if (!parseChord(name)) return setError(true);
    onPick(name);
  };
  return (
    <div className="edit-tools chord-picker" role="group" aria-label="Chord">
      <span className="label">
        Bar {at.bar + 1}, beat {at.beat + 1}
      </span>
      <input
        className={`bar-input ${error ? 'invalid' : ''}`}
        value={text}
        autoFocus
        aria-label="Chord from here"
        aria-invalid={error}
        spellCheck={false}
        onChange={(e) => (setText(e.target.value), setError(false))}
        onKeyDown={(e) => (e.key === 'Enter' ? apply() : e.key === 'Escape' && onCancel())}
      />
      <button className="btn btn-primary" onClick={apply}>
        Set
      </button>
      {picks.map((name) => (
        <button key={name} className="btn btn-ghost" onClick={() => onPick(name)}>
          {name}
        </button>
      ))}
      <button className="btn btn-ghost" onClick={() => onPick(null)}>
        No chord
      </button>
      <button className="btn btn-ghost" onClick={onCancel}>
        Cancel
      </button>
    </div>
  );
}

interface View {
  shown: Timeline | null;
  starts: number[];
  /** The bar each bar line starts. */
  barOf: Map<number, number>;
  blocks: ChordBlock[];
  pps: number;
  sel: BeatRange | null;
  envelope: Uint8Array | null;
  chordEdit: ChordBlock | null;
  playing: boolean;
}

function draw(canvas: HTMLCanvasElement, scroller: HTMLDivElement, v: View): void {
  const width = scroller.clientWidth;
  const dpr = window.devicePixelRatio || 1;
  if (canvas.width !== Math.round(width * dpr) || canvas.height !== Math.round(HEIGHT * dpr)) {
    canvas.width = Math.round(width * dpr);
    canvas.height = Math.round(HEIGHT * dpr);
    canvas.style.width = `${width}px`;
    canvas.style.height = `${HEIGHT}px`;
  }
  const ctx = canvas.getContext('2d');
  if (!ctx || !v.shown) return;
  const { pps, starts, barOf, blocks, sel, envelope } = v;
  const times = v.shown.beatTimes;
  const left = scroller.scrollLeft;
  const xOf = (t: number) => t * pps - left;
  ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
  ctx.fillStyle = color.bg.stage;
  ctx.fillRect(0, 0, width, HEIGHT);

  // The level.
  if (envelope) {
    ctx.beginPath();
    ctx.moveTo(0, LEVEL_BOTTOM);
    for (let x = 0; x <= width; x += 2) {
      const level = envelope[Math.floor(((left + x) / pps) * ENVELOPE_FPS)] ?? 0;
      ctx.lineTo(x, LEVEL_BOTTOM - (level / 255) * (LEVEL_BOTTOM - LEVEL_TOP));
    }
    ctx.lineTo(width, LEVEL_BOTTOM);
    ctx.closePath();
    ctx.fillStyle = color.surface.hover;
    ctx.fill();
  }

  // Beat lines, bar lines brighter with their numbers.
  const barGap = starts.length > 1 ? (times[starts[1]!] ?? 0) - (times[0] ?? 0) : 2;
  const numberEvery = Math.max(1, Math.ceil(28 / Math.max(1, barGap * pps)));
  ctx.font = `600 11px ${font.family.display}`;
  ctx.textBaseline = 'top';
  const from = Math.max(0, nearestBeat(times, (left - 40) / pps) - 1);
  for (let i = from; i < times.length; i++) {
    const x = xOf(times[i]!);
    if (x > width + 40) break;
    const bar = barOf.get(i);
    const isLine = bar !== undefined;
    const selected = sel !== null && i >= sel.first && i <= sel.last;
    ctx.fillStyle = selected ? color.primary.default : isLine ? color.accent.default : color.border.strong;
    ctx.fillRect(Math.round(x) - (isLine || selected ? 1 : 0.5), LEVEL_TOP - 4, isLine || selected ? 2 : 1, CHORD_TOP - LEVEL_TOP + 4);
    if (isLine && bar % numberEvery === 0) {
      ctx.fillStyle = color.text.accent;
      ctx.fillText(String(bar + 1), x + 3, 3);
    }
  }

  // Chord blocks.
  ctx.font = `700 13px ${font.family.display}`;
  ctx.textBaseline = 'middle';
  for (const b of blocks) {
    const from = times[b.start];
    if (from === undefined) break;
    const x0 = xOf(from) + 1;
    const x1 = xOf(times[b.end] ?? times[times.length - 1]!) - 1;
    if (x1 < 0 || x0 > width) continue;
    const editing = v.chordEdit?.start === b.start;
    ctx.fillStyle = b.name ? color.surface.raised : color.bg.canvas;
    ctx.fillRect(x0, CHORD_TOP, x1 - x0, CHORD_BOTTOM - CHORD_TOP);
    ctx.strokeStyle = editing ? color.primary.default : color.border.default;
    ctx.lineWidth = editing ? 2 : 1;
    ctx.strokeRect(x0 + 0.5, CHORD_TOP + 0.5, x1 - x0 - 1, CHORD_BOTTOM - CHORD_TOP - 1);
    if (x1 - x0 > 18) {
      ctx.save();
      ctx.beginPath();
      ctx.rect(x0, CHORD_TOP, x1 - x0, CHORD_BOTTOM - CHORD_TOP);
      ctx.clip();
      ctx.fillStyle = b.name ? color.text.primary : color.text.disabled;
      ctx.fillText(b.name ?? 'N.C.', Math.max(x0, 0) + 6, (CHORD_TOP + CHORD_BOTTOM) / 2);
      ctx.restore();
    }
  }

  // The playhead, kept in view.
  const clock = live.songClock;
  const now = performance.now();
  if (v.playing && now - clock.ms < 3000 && times[clock.beat] !== undefined) {
    const t = times[clock.beat]! + (now - clock.ms) / 1000;
    const x = xOf(t);
    ctx.fillStyle = color.primary.default;
    ctx.fillRect(Math.round(x) - 1, 0, 2, HEIGHT);
    if (x > width * 0.85 || x < 0) scroller.scrollLeft = t * pps - width * 0.25;
  }
}
