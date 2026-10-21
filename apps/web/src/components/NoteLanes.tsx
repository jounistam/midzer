import { Application, BlurFilter, Container, Graphics, Text } from 'pixi.js';
import { useEffect, useRef } from 'react';
import { barLines, placements } from '../learn/lanes';
import { practiceLive } from '../learn/practice';
import type { TargetState } from '../learn/judge';
import { color, font, glow as glowTokens, hex, NOTE_ALPHA, radius } from '../theme';
import { layoutKeys, type KeyRect } from './keyboardLayout';

/** Beats shown above the hit line, and below it while notes still sound. */
const AHEAD_BEATS = 4;
const BEHIND_BEATS = 0.25;
const HIT_LINE_INSET = 6;
const FLASH_MS = 260;
const MAX_LABELS = 24;

interface NoteLanesProps {
  /** Same range as the keyboard below, so the lanes line up with its keys. */
  low?: number;
  high?: number;
}

const stateColor = (state: TargetState, kind: 'note' | 'chord') => {
  if (state === 'open') return hex(kind === 'chord' ? color.note.chord : color.note.melody);
  if (state === 'miss' || state === 'skipped') return hex(color.text.disabled);
  return hex(color.status.success);
};

/**
 * GPU-drawn falling notes (PixiJS) for Learn: the targets come down their
 * keys' lanes and reach the hit line when they are due. Played ones turn
 * green, missed ones grey; the hit line pulses while the band waits. Fills
 * the height its container gives it.
 */
export function NoteLanes({ low = 36, high = 96 }: NoteLanesProps) {
  const hostRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    const host = hostRef.current!;
    const app = new Application();
    let keys = new Map<number, KeyRect>();
    let width = 0;
    let height = 0;
    let hitY = 0;
    let pxPerBeat = 1;
    let disposed = false;
    let initialised = false;

    const lanes = new Graphics();
    const bars = new Graphics();
    const notes = new Graphics();
    const glow = new Graphics();
    glow.filters = [new BlurFilter({ strength: glowTokens.note.blur, quality: 2 })];
    glow.blendMode = 'add';
    const hitLine = new Graphics();
    const hitGlow = new Graphics();
    hitGlow.filters = [new BlurFilter({ strength: glowTokens.beatPulse.blur, quality: 2 })];
    hitGlow.blendMode = 'add';
    const labels = new Container();

    const layout = (w: number, h: number) => {
      width = w;
      height = h;
      hitY = h - HIT_LINE_INSET;
      pxPerBeat = (hitY - 8) / AHEAD_BEATS;
      keys = new Map(layoutKeys(low, high, w, h).map((k) => [k.note, k]));
      lanes.clear();
      for (const k of keys.values()) {
        if (k.black) lanes.rect(k.x, 0, k.w, height).fill({ color: hex(color.surface.default), alpha: 0.7 });
        else if (k.note % 12 === 0) lanes.rect(k.x, 0, 1, height).fill({ color: hex(color.border.default), alpha: 0.8 });
      }
      hitLine.clear().rect(0, hitY - 1, w, 2).fill(hex(color.primary.default));
      hitGlow.clear().rect(0, hitY - 3, w, 6).fill({ color: hex(glowTokens.beatPulse.color), alpha: glowTokens.beatPulse.alpha });
    };

    const label = (i: number) => {
      while (labels.children.length <= i) {
        labels.addChild(
          new Text({
            text: '',
            style: { fontFamily: font.family.display, fontSize: 14, fontWeight: '700', fill: hex(color.text.accent) },
          }),
        );
      }
      return labels.children[i] as Text;
    };

    const draw = () => {
      const now = performance.now();
      const { clock, judge, targets, loop, flashes } = practiceLive;
      const pos = clock.position(now) ?? practiceLive.startBeat;
      const y = (beats: number) => hitY - beats * pxPerBeat;

      bars.clear();
      for (const b of barLines(pos - BEHIND_BEATS, pos + AHEAD_BEATS)) {
        bars.rect(0, Math.round(y(b - pos)), width, 1).fill({ color: hex(color.border.strong), alpha: 0.6 });
      }

      notes.clear();
      glow.clear();
      let used = 0;
      for (const p of placements(targets, pos, AHEAD_BEATS, BEHIND_BEATS, loop)) {
        const t = p.target;
        const state: TargetState = p.nextPass ? 'open' : (judge?.states[t.id] ?? 'open');
        const fill = stateColor(state, t.kind);
        const alpha = state === 'open' ? NOTE_ALPHA : state === 'miss' ? 0.35 : 0.6;
        const top = y(p.offset + t.len) + 2;
        const bottom = Math.min(y(p.offset), hitY);
        if (bottom - top < 2) continue;
        let left = Infinity;
        for (const key of t.keys) {
          const k = keys.get(key);
          if (!k) continue;
          left = Math.min(left, k.x);
          notes.roundRect(k.x + 1, top, k.w - 2, bottom - top, radius.sm).fill({ color: fill, alpha });
          if (state === 'open' && p.offset < 0.5) {
            glow.roundRect(k.x + 1, Math.max(top, bottom - 24), k.w - 2, Math.min(24, bottom - top), radius.sm).fill({ color: fill, alpha: glowTokens.note.alpha });
          }
        }
        if (t.kind === 'chord' && t.name && left < Infinity && used < MAX_LABELS && bottom - top > 20) {
          const text = label(used++);
          if (text.text !== t.name) text.text = t.name;
          text.style.fill = state === 'open' ? hex(color.text.accent) : hex(color.text.muted);
          text.position.set(Math.max(4, left - text.width - 8), bottom - 20);
          text.visible = true;
        }
      }
      for (let i = used; i < labels.children.length; i++) labels.children[i]!.visible = false;

      // A burst on the hit line where a target was just played.
      for (const [id, at] of flashes) {
        const age = now - at;
        if (age > FLASH_MS) {
          flashes.delete(id);
          continue;
        }
        const t = targets[id];
        if (!t) continue;
        for (const key of t.keys) {
          const k = keys.get(key);
          if (k) glow.rect(k.x - 4, hitY - 18, k.w + 8, 22).fill({ color: hex(color.status.success), alpha: 1 - age / FLASH_MS });
        }
      }

      const waiting = clock.holding(now);
      hitGlow.alpha = waiting ? 0.55 + 0.45 * Math.sin(now / 160) : 0.5;
    };

    const resize = new ResizeObserver(([entry]) => {
      if (!initialised || !entry) return;
      const w = Math.floor(entry.contentRect.width);
      const h = Math.floor(entry.contentRect.height);
      app.renderer.resize(w, h);
      layout(w, h);
    });

    void app
      .init({
        width: host.clientWidth || 800,
        height: host.clientHeight || 240,
        backgroundAlpha: 0,
        antialias: true,
        autoDensity: true,
        resolution: window.devicePixelRatio || 1,
      })
      .then(() => {
        if (disposed) {
          app.destroy(true, { children: true });
          return;
        }
        initialised = true;
        app.canvas.style.display = 'block';
        host.appendChild(app.canvas);
        app.stage.addChild(lanes, bars, notes, glow, hitGlow, hitLine, labels);
        layout(host.clientWidth || 800, host.clientHeight || 240);
        app.ticker.add(draw);
        resize.observe(host);
      });

    return () => {
      disposed = true;
      resize.disconnect();
      if (initialised) app.destroy(true, { children: true });
    };
  }, [low, high]);

  return <div ref={hostRef} className="note-lanes" role="img" aria-label="Falling notes" />;
}
