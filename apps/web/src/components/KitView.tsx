import { DRUM_PIECES } from '@midzer/audio-web';
import { Application, BlurFilter, Container, Graphics, Text } from 'pixi.js';
import { useEffect, useRef } from 'react';
import { PIECE_INFO } from '../drums/kit';
import { layoutKit, pieceAt, type Pad } from '../drums/kitLayout';
import { live } from '../live';
import { TAP_HINT } from '../playalong/solo';
import { drumHit } from '../session';
import { color, font, glow as glowTokens, hex, NOTE_ALPHA } from '../theme';

/** How long a hit's light takes to fade, in ms. */
const DECAY_MS = 220;
const PIECE_INDEX = new Map(DRUM_PIECES.map((p, i) => [p.id, i]));

/**
 * GPU-drawn drum kit seen from above (PixiJS). Pieces light up with each hit,
 * brighter for harder hits, and can be played with mouse or touch. Pads a
 * drum solo asks for (`live.drumHint`) are ringed.
 */
export function KitView({ height = 200 }: { height?: number }) {
  const hostRef = useRef<HTMLDivElement>(null);
  const lastRef = useRef<HTMLParagraphElement>(null);

  useEffect(() => {
    const host = hostRef.current!;
    const app = new Application();
    let pads: Pad[] = [];
    let disposed = false;
    let initialised = false;
    let lit = false;
    let shownHit = -Infinity;

    const glow = new Graphics();
    glow.filters = [new BlurFilter({ strength: glowTokens.heldKey.blur, quality: 3 })];
    glow.blendMode = 'add';
    const base = new Graphics();
    const hits = new Graphics();
    const labels = new Container();

    /** Rings sit around the pad on the same spot; this is that pad's radius. */
    const innerRadius = (ring: Pad) => pads.find((p) => p !== ring && p.x === ring.x && p.y === ring.y)?.r ?? 0;
    const tint = (pad: Pad) => hex(pad.shape === 'drum' || pad.piece === 'snareRim' ? color.note.chord : color.note.melody);

    /** Draws a pad's outline with `paint`; a ring then loses its middle. */
    const shape = (g: Graphics, pad: Pad, paint: (g: Graphics) => void) => {
      if (pad.shape === 'pedal') g.roundRect(pad.x - pad.r, pad.y - pad.r * 0.6, pad.r * 2, pad.r * 1.2, pad.r * 0.3);
      else g.circle(pad.x, pad.y, pad.r);
      paint(g);
      if (pad.shape === 'ring') g.circle(pad.x, pad.y, innerRadius(pad)).cut();
    };

    const drawBase = () => {
      base.clear();
      for (const pad of pads) {
        const drum = pad.shape === 'drum';
        shape(base, pad, (g) =>
          g
            .fill(hex(drum ? color.surface.raised : color.surface.default))
            .stroke({ width: drum ? 3 : 1.5, color: hex(drum ? color.border.strong : color.border.default) }),
        );
      }
    };

    const drawHits = (now: number) => {
      hits.clear();
      glow.clear();
      lit = false;
      for (const pad of pads) {
        // To tap: a bright ring and a tint; the player's part: a thin ring.
        const hint = live.drumHint[PIECE_INDEX.get(pad.piece)!]!;
        if (hint === 0) continue;
        const tap = hint === TAP_HINT;
        hits.circle(pad.x, pad.y, pad.r + (tap ? 3 : 2)).stroke({ width: tap ? 3 : 1.5, color: hex(color.note.melody), alpha: tap ? 1 : 0.7 });
        if (tap) shape(hits, pad, (g) => g.fill({ color: hex(color.note.melody), alpha: 0.12 }));
      }
      for (const pad of pads) {
        const i = PIECE_INDEX.get(pad.piece)!;
        const age = now - live.drumTime[i]!;
        if (!(age >= 0 && age < DECAY_MS * 4)) continue;
        lit = true;
        const strength = (0.35 + (0.65 * live.drumVelocity[i]!) / 127) * Math.exp(-age / DECAY_MS);
        shape(hits, pad, (g) => g.fill({ color: tint(pad), alpha: NOTE_ALPHA * strength }));
        shape(glow, pad, (g) => g.fill({ color: tint(pad), alpha: glowTokens.heldKey.alpha * strength }));
      }
      const pedal = pads.find((p) => p.piece === 'hiHatPedal');
      if (pedal && live.hiHat >= 0) {
        // The pedal position as the kit sends it, as a bar under the pedal.
        const w = pedal.r * 2;
        hits.roundRect(pedal.x - pedal.r, pedal.y + pedal.r * 0.75, w, 4, 2).fill({ color: hex(color.surface.track) });
        hits.roundRect(pedal.x - pedal.r, pedal.y + pedal.r * 0.75, (w * live.hiHat) / 127, 4, 2).fill(hex(color.accent.default));
      }
    };

    const showLastHit = () => {
      let latest = -1;
      for (let i = 0; i < DRUM_PIECES.length; i++) if (latest < 0 || live.drumTime[i]! > live.drumTime[latest]!) latest = i;
      const t = live.drumTime[latest]!;
      if (t === shownHit || !lastRef.current) return;
      shownHit = t;
      lastRef.current.textContent =
        t === -Infinity ? 'Hit a drum' : `${PIECE_INFO[DRUM_PIECES[latest]!.id].label} · ${live.drumVelocity[latest]}`;
    };

    const layout = (width: number) => {
      pads = layoutKit(width, height);
      labels.removeChildren().forEach((c) => c.destroy());
      const size = Math.max(10, Math.min(13, height / 16));
      for (const pad of pads) {
        const info = PIECE_INFO[pad.piece];
        const label = new Text({
          text: `${info.short} ${info.key.toUpperCase()}`,
          style: { fontFamily: font.family.body, fontSize: size, fontWeight: '600', fill: hex(color.text.muted) },
        });
        label.anchor.set(0.5);
        // Ring labels go on the ring, above the pad inside it.
        const y = pad.shape === 'ring' ? pad.y - (pad.r + innerRadius(pad)) / 2 : pad.y;
        label.position.set(pad.x, y);
        labels.addChild(label);
      }
      drawBase();
      drawHits(performance.now());
    };

    const press = (e: PointerEvent) => {
      const r = host.getBoundingClientRect();
      const piece = pieceAt(pads, e.clientX - r.left, e.clientY - r.top);
      if (piece) drumHit(piece, 100);
    };
    host.addEventListener('pointerdown', press);

    const resize = new ResizeObserver(([entry]) => {
      if (!initialised || !entry) return;
      const width = Math.floor(entry.contentRect.width);
      app.renderer.resize(width, height);
      layout(width);
    });

    let drawnVersion = -1;
    void app
      .init({
        width: host.clientWidth || 800,
        height,
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
        app.stage.addChild(base, hits, glow, labels);
        layout(host.clientWidth || 800);
        let drawnHiHat = live.hiHat;
        app.ticker.add(() => {
          if (!lit && live.version === drawnVersion && live.hiHat === drawnHiHat) return;
          drawnVersion = live.version;
          drawnHiHat = live.hiHat;
          drawHits(performance.now());
          showLastHit();
        });
        resize.observe(host);
      });

    return () => {
      disposed = true;
      resize.disconnect();
      host.removeEventListener('pointerdown', press);
      if (initialised) app.destroy(true, { children: true });
    };
  }, [height]);

  return (
    <div className="kit-view">
      <div ref={hostRef} className="kit-canvas" style={{ height }} role="img" aria-label="Drum kit" />
      <p className="kit-last" ref={lastRef}>
        Hit a drum
      </p>
    </div>
  );
}
