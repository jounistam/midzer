import { useEffect, useRef } from 'react';
import { live } from '../live';
import { PPQ, TICKS_PER_BAR, type SoloTake } from '../playalong/replay';
import { color } from '../theme';

/** Each row's height, and how many bars a row holds on a wide and a narrow card. */
const ROW_HEIGHT = 64;
const ROW_GAP = 8;
const WIDE_BARS = 4;
const NARROW_BARS = 2;
const NARROW_PX = 480;
/** The fewest semitones shown, so a solo on three notes doesn't fill the rows. */
const MIN_RANGE = 12;

interface SoloRollProps {
  take: SoloTake;
  /** Replaying at this tempo: the playhead runs from `live.replayClock`. */
  playingBpm: number | null;
}

/**
 * A recorded solo, bar by bar in rows like a score: the player's own notes in
 * magenta, midzer's in cyan, and while it replays a playhead. Drawn on a
 * small canvas as wide as its container.
 */
export function SoloRoll({ take, playingBpm }: SoloRollProps) {
  const hostRef = useRef<HTMLDivElement>(null);
  const canvasRef = useRef<HTMLCanvasElement>(null);

  useEffect(() => {
    const host = hostRef.current;
    const canvas = canvasRef.current;
    if (!host || !canvas) return;
    let frame = 0;
    const redraw = () => draw(canvas, host.clientWidth, take, playingBpm);
    const tick = () => {
      redraw();
      frame = requestAnimationFrame(tick);
    };
    if (playingBpm !== null) tick();
    else redraw();
    const resize = new ResizeObserver(redraw);
    resize.observe(host);
    return () => {
      cancelAnimationFrame(frame);
      resize.disconnect();
    };
  }, [take, playingBpm]);

  return (
    <div ref={hostRef} className="solo-roll">
      <canvas ref={canvasRef} role="img" aria-label={`Your solo: ${take.notes.length} notes over ${take.bars} bars`} />
    </div>
  );
}

function draw(canvas: HTMLCanvasElement, width: number, take: SoloTake, playingBpm: number | null): void {
  const perRow = width < NARROW_PX ? NARROW_BARS : WIDE_BARS;
  const rows = Math.ceil(take.bars / perRow);
  const height = rows * ROW_HEIGHT + (rows - 1) * ROW_GAP;
  const dpr = window.devicePixelRatio || 1;
  if (canvas.width !== Math.round(width * dpr) || canvas.height !== Math.round(height * dpr)) {
    canvas.width = Math.round(width * dpr);
    canvas.height = Math.round(height * dpr);
    canvas.style.width = `${width}px`;
    canvas.style.height = `${height}px`;
  }
  const ctx = canvas.getContext('2d');
  if (!ctx) return;
  ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
  ctx.clearRect(0, 0, width, height);

  const rowTicks = perRow * TICKS_PER_BAR;
  const xOf = (tick: number) => ((tick % rowTicks) / rowTicks) * width;
  const topOf = (tick: number) => Math.floor(tick / rowTicks) * (ROW_HEIGHT + ROW_GAP);
  const keys = take.notes.map((n) => n.key);
  const middle = (Math.min(...keys) + Math.max(...keys)) / 2;
  const range = Math.max(MIN_RANGE, Math.max(...keys) - Math.min(...keys) + 2);
  const low = middle - range / 2;
  const noteHeight = Math.min(6, Math.max(2, ROW_HEIGHT / range));
  const yOf = (key: number) => ROW_HEIGHT - ((key - low) / range) * ROW_HEIGHT - noteHeight / 2;

  for (let row = 0; row < rows; row++) {
    const top = row * (ROW_HEIGHT + ROW_GAP);
    ctx.fillStyle = color.bg.stage;
    ctx.fillRect(0, top, width, ROW_HEIGHT);
    // Beat lines, bar lines brighter.
    for (let beat = 0; beat < perRow * 4; beat++) {
      const bar = beat % 4 === 0;
      ctx.fillStyle = bar ? color.border.strong : color.border.default;
      ctx.fillRect(Math.round((beat / (perRow * 4)) * width), top, bar ? 2 : 1, ROW_HEIGHT);
    }
  }

  for (const n of take.notes) {
    // A note over a row's end goes on in the next row.
    let tick = n.tick;
    const end = n.tick + n.len;
    for (;;) {
      const rowEnd = (Math.floor(tick / rowTicks) + 1) * rowTicks;
      const to = Math.min(end, rowEnd);
      ctx.fillStyle = n.own ? color.note.melody : color.note.chord;
      ctx.globalAlpha = n.own ? 1 : 0.7;
      ctx.fillRect(xOf(tick), topOf(tick) + yOf(n.key), Math.max(2, ((to - tick) / rowTicks) * width - 1), noteHeight);
      if (to >= end) break;
      tick = to;
    }
  }
  ctx.globalAlpha = 1;

  const clock = live.replayClock;
  if (playingBpm !== null && clock.ms > -Infinity) {
    const tick = ((performance.now() - clock.ms) / 60_000) * playingBpm * PPQ;
    if (tick >= 0 && tick < take.bars * TICKS_PER_BAR) {
      ctx.fillStyle = color.text.primary;
      ctx.fillRect(Math.round(xOf(tick)) - 1, topOf(tick), 2, ROW_HEIGHT);
    }
  }
}
