import { Application, BlurFilter, Container, Graphics, Text } from 'pixi.js';
import { useEffect, useRef } from 'react';
import { live } from '../live';
import { playerNoteOff, playerNoteOn } from '../session';
import { color, font, glow as glowTokens, hex, NOTE_ALPHA } from '../theme';
import { boundaryX, keyAt, layoutKeys, type KeyRect } from './keyboardLayout';

interface KeyboardProps {
  low?: number;
  high?: number;
  /** Notes below this form chords (cyan); 0 = whole keyboard. */
  split: number;
  height?: number;
}

/**
 * GPU-drawn keyboard (PixiJS). Shows the player's keys in chord/melody colours,
 * the band's notes as a glow and the keys Learn asks for next, and can be
 * played with mouse or touch.
 */
export function Keyboard({ low = 36, high = 96, split, height = 200 }: KeyboardProps) {
  const hostRef = useRef<HTMLDivElement>(null);
  const splitRef = useRef(split);
  splitRef.current = split;

  useEffect(() => {
    const host = hostRef.current!;
    const app = new Application();
    let keys: KeyRect[] = [];
    let disposed = false;
    let initialised = false;
    let drawnVersion = -1;
    let drawnSplit = -1;

    const glow = new Graphics();
    glow.filters = [new BlurFilter({ strength: glowTokens.heldKey.blur, quality: 3 })];
    glow.blendMode = 'add';
    const whiteLayer = new Graphics();
    const blackLayer = new Graphics();
    const markers = new Graphics();
    const splitGlow = new Graphics();
    splitGlow.filters = [new BlurFilter({ strength: glowTokens.splitLine.blur, quality: 2 })];
    splitGlow.blendMode = 'add';
    const labels = new Container();
    const zoneLabels = new Container();

    const draw = () => {
      const s = splitRef.current;
      drawnVersion = live.version;
      drawnSplit = s;
      const keyColor = (note: number) => hex(s > 0 && note >= s ? color.note.melody : color.note.chord);
      glow.clear();
      whiteLayer.clear();
      blackLayer.clear();
      markers.clear();
      splitGlow.clear();

      for (const k of keys) {
        const held = live.player[k.note]! > 0;
        const band = live.band[k.note]! > 0;
        const layer = k.black ? blackLayer : whiteLayer;
        // States from the Figma "Keyboard key" component: idle, band-playing, user-held.
        const base = band
          ? hex(k.black ? color.key.bandBlack : color.key.bandWhite)
          : hex(k.black ? color.key.black : color.key.white);
        if (k.black) {
          layer.roundRect(k.x, k.y - 4, k.w, k.h + 4, 3).fill(base);
        } else {
          layer.rect(k.x, k.y, k.w, k.h).fill(base).rect(k.x + k.w - 1, k.y, 1, k.h).fill({ color: hex(color.key.label), alpha: 0.35 });
        }
        const hint = live.hint[k.note]!;
        if (hint === 3 && !held) {
          // Playing along: a key of the chord coming next, to move towards.
          layer.roundRect(k.x + 3, k.y + k.h - 10, k.w - 6, 6, 3).stroke({ color: hex(color.note.chord), width: 1.5, alpha: NOTE_ALPHA });
        } else if ((hint === 5 || hint === 6) && !held) {
          // Soloing: the scale's keys marked, the chord's tones lit brighter.
          const tint = hex(color.note.melody);
          if (hint === 6) layer.roundRect(k.x, k.y - (k.black ? 4 : 0), k.w, k.h + (k.black ? 4 : 0), k.black ? 3 : 0).fill({ color: tint, alpha: 0.18 });
          const bar = layer.roundRect(k.x + 3, k.y + k.h - 10, k.w - 6, 6, 3);
          if (hint === 6) bar.fill({ color: tint, alpha: NOTE_ALPHA });
          else bar.stroke({ color: tint, width: 1.5, alpha: NOTE_ALPHA });
        } else if (hint && !held) {
          // Learn: the key to play next, in its part's colour.
          const tint = hex(hint === 2 ? color.note.chord : color.note.melody);
          layer.roundRect(k.x, k.y - (k.black ? 4 : 0), k.w, k.h + (k.black ? 4 : 0), k.black ? 3 : 0).fill({ color: tint, alpha: 0.3 });
          layer.roundRect(k.x + 3, k.y + k.h - 10, k.w - 6, 6, 3).fill({ color: tint, alpha: NOTE_ALPHA });
        }
        if (held) {
          const radius = k.black ? 3 : 0;
          layer.roundRect(k.x, k.y - (k.black ? 4 : 0), k.w, k.h + (k.black ? 4 : 0), radius).fill({ color: keyColor(k.note), alpha: NOTE_ALPHA });
          glow.rect(k.x, k.y, k.w, k.h * 0.5).fill({ color: keyColor(k.note), alpha: glowTokens.heldKey.alpha });
        }
      }

      const sx = s > 0 ? boundaryX(keys, s) : null;
      zoneLabels.visible = sx !== null;
      if (sx !== null) {
        markers.rect(sx - 1, 0, 2, keys[0]?.h ?? 0).fill(hex(color.key.split));
        splitGlow.rect(sx - 2, 0, 4, keys[0]?.h ?? 0).fill({ color: hex(glowTokens.splitLine.color), alpha: glowTokens.splitLine.alpha });
        const [chords, melody] = zoneLabels.children as Container[];
        chords!.position.set(sx - 8 - chords!.width, (keys[0]?.h ?? 0) - 52);
        melody!.position.set(sx + 8, (keys[0]?.h ?? 0) - 52);
      }
    };

    const pill = (text: string) => {
      const c = new Container();
      const t = new Text({
        text,
        style: { fontFamily: font.family.body, fontSize: 12, fontWeight: '600', fill: hex(color.text.onPrimary) },
      });
      const bg = new Graphics().roundRect(0, 0, t.width + 16, 22, 11).fill(hex(color.primary.default));
      t.position.set(8, 3);
      c.addChild(bg, t);
      return c;
    };

    const layout = (width: number) => {
      keys = layoutKeys(low, high, width, height);
      labels.removeChildren().forEach((c) => c.destroy());
      for (const k of keys) {
        if (k.black || k.note % 12 !== 0) continue;
        const label = new Text({
          text: `C${Math.floor(k.note / 12) - 1}`,
          style: { fontFamily: font.family.body, fontSize: 11, fill: hex(color.key.label) },
        });
        label.position.set(k.x + 6, k.h - 20);
        labels.addChild(label);
      }
      draw();
    };

    // Pointer playing, one note per pointer so multi-touch chords work.
    const pointers = new Map<number, number>();
    const toLocal = (e: PointerEvent) => {
      const r = host.getBoundingClientRect();
      return keyAt(keys, e.clientX - r.left, e.clientY - r.top);
    };
    const press = (e: PointerEvent) => {
      const note = toLocal(e);
      if (note === null) return;
      host.setPointerCapture(e.pointerId);
      pointers.set(e.pointerId, note);
      playerNoteOn(note, 100);
    };
    const move = (e: PointerEvent) => {
      const prev = pointers.get(e.pointerId);
      if (prev === undefined) return;
      const note = toLocal(e);
      if (note === prev) return;
      playerNoteOff(prev);
      if (note === null) {
        pointers.delete(e.pointerId);
      } else {
        pointers.set(e.pointerId, note);
        playerNoteOn(note, 100);
      }
    };
    const release = (e: PointerEvent) => {
      const note = pointers.get(e.pointerId);
      if (note !== undefined) playerNoteOff(note);
      pointers.delete(e.pointerId);
    };
    host.addEventListener('pointerdown', press);
    host.addEventListener('pointermove', move);
    host.addEventListener('pointerup', release);
    host.addEventListener('pointercancel', release);

    const resize = new ResizeObserver(([entry]) => {
      if (!initialised || !entry) return;
      const width = Math.floor(entry.contentRect.width);
      app.renderer.resize(width, height);
      layout(width);
    });

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
        zoneLabels.addChild(pill('Chords'), pill('Melody'));
        app.stage.addChild(whiteLayer, blackLayer, glow, splitGlow, markers, labels, zoneLabels);
        layout(host.clientWidth || 800);
        app.ticker.add(() => {
          if (live.version !== drawnVersion || splitRef.current !== drawnSplit) draw();
        });
        resize.observe(host);
      });

    return () => {
      disposed = true;
      resize.disconnect();
      host.removeEventListener('pointerdown', press);
      host.removeEventListener('pointermove', move);
      host.removeEventListener('pointerup', release);
      host.removeEventListener('pointercancel', release);
      for (const note of pointers.values()) playerNoteOff(note);
      if (initialised) app.destroy(true, { children: true });
    };
  }, [low, high, height]);

  return <div ref={hostRef} className="keyboard" style={{ height }} role="img" aria-label="Keyboard" />;
}
