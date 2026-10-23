import { Application, BlurFilter, Container, Graphics, Text } from 'pixi.js';
import { useEffect, useRef } from 'react';
import { live } from '../live';
import { NOTE_NAMES } from '@midzer/audio-web';
import { chordBlocks, snapToBeats } from '../playalong/chords';
import { voicing, type AlongLevel } from '../playalong/levels';
import { liveChordWindow, useSession } from '../session';
import { color, font, glow as glowTokens, hex, NOTE_ALPHA, radius } from '../theme';
import { layoutKeys, type KeyRect } from './keyboardLayout';

/** Beats shown above the hit line. */
const AHEAD_BEATS = 6;
/** A beat's length before the tempo is found. */
const FALLBACK_BEAT_MS = 600;
/** Where the hit line sits, down the lanes; the chords heard move away below it. */
const HIT_AT = 0.6;
/** How much slower the chords heard move than the ones coming, so they stay in view. */
const PAST_SLOWER = 3;
const MAX_LABELS = 24;

interface ChordLanesProps {
  /** Same range as the keyboard below, so the lanes line up with its keys. */
  low?: number;
  high?: number;
}

/**
 * GPU-drawn falling chords (PixiJS) for playing along on keys, like Learn's
 * falling notes: the keys of each chord at the player's level come down their
 * lanes and reach the hit line when the chord starts, left hand in magenta,
 * right in cyan, each with its note name. Chords fall once the song's loop
 * says what comes next, their changes on the beat lines. The hit line sits
 * well above the keyboard: below it every chord heard moves slowly away,
 * each chord one whole block through the line, so the one sounding now and
 * the ones just heard stay in view.
 * Fills the height its container gives it.
 */
export function ChordLanes({ low = 36, high = 96 }: ChordLanesProps) {
  const hostRef = useRef<HTMLDivElement>(null);
  const level = useSession((s) => s.alongLevel);
  const bpm = useSession((s) => s.bpm);
  const locked = useSession((s) => s.liveSource.status === 'on' && s.liveTracker.locked);
  const listening = useSession((s) => s.liveSource.status === 'on');
  const state = useRef({ level, bpm, locked, listening });
  state.current = { level, bpm, locked, listening };

  useEffect(() => {
    const host = hostRef.current!;
    const app = new Application();
    let keys = new Map<number, KeyRect>();
    let width = 0;
    let height = 0;
    let hitY = 0;
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
    const names = new Container();
    const letters = new Container();
    const message = new Text({ text: '', style: { fontFamily: font.family.body, fontSize: 14, fill: hex(color.text.muted) } });

    const pooled = (pool: Container, i: number, make: () => Text) => {
      while (pool.children.length <= i) pool.addChild(make());
      const t = pool.children[i] as Text;
      t.visible = true;
      return t;
    };
    const nameText = () => new Text({ text: '', style: { fontFamily: font.family.display, fontSize: 16, fontWeight: '700', fill: hex(color.text.accent) } });
    const letterText = () => new Text({ text: '', style: { fontFamily: font.family.body, fontSize: 11, fontWeight: '700', fill: hex(color.text.onAccent) } });
    const setText = (t: Text, s: string) => {
      if (t.text !== s) t.text = s;
    };

    const layout = (w: number, h: number) => {
      width = w;
      height = h;
      hitY = Math.round(h * HIT_AT);
      keys = new Map(layoutKeys(low, high, w, h).map((k) => [k.note, k]));
      lanes.clear();
      lanes.rect(0, hitY, w, h - hitY).fill({ color: hex(color.surface.default), alpha: 0.35 });
      for (const k of keys.values()) {
        if (k.black) lanes.rect(k.x, 0, k.w, height).fill({ color: hex(color.surface.default), alpha: 0.7 });
        else if (k.note % 12 === 0) lanes.rect(k.x, 0, 1, height).fill({ color: hex(color.border.default), alpha: 0.8 });
      }
      hitLine.clear().rect(0, hitY - 1, w, 2).fill(hex(color.primary.default));
      hitGlow.clear().rect(0, hitY - 3, w, 6).fill({ color: hex(glowTokens.beatPulse.color), alpha: glowTokens.beatPulse.alpha });
    };

    const draw = () => {
      const { level, bpm, locked, listening } = state.current;
      const now = performance.now();
      const beatMs = locked && bpm > 0 ? 60_000 / bpm : FALLBACK_BEAT_MS;
      const pxAhead = (hitY - 8) / (AHEAD_BEATS * beatMs);
      const pxBehind = pxAhead / PAST_SLOWER;
      const behindMs = (height - hitY) / pxBehind;
      // Coming chords fall onto the hit line; heard ones carry on below it, slower.
      const y = (ms: number) => (ms >= now ? hitY - (ms - now) * pxAhead : hitY + (now - ms) * pxBehind);

      // Beats, and brighter bar lines on the 1s, carried on from the last beat heard.
      bars.clear();
      const clock = live.beatClock;
      const sinceBeat = now - clock.ms;
      if (locked && sinceBeat < 4 * beatMs) {
        for (let k = -Math.ceil((sinceBeat + behindMs) / beatMs); clock.ms + k * beatMs <= now + AHEAD_BEATS * beatMs; k++) {
          const at = clock.ms + k * beatMs;
          const one = (((clock.beat + k) % 4) + 4) % 4 === 0;
          const fade = at < now ? 0.5 : 1;
          bars.rect(0, Math.round(y(at)), width, 1).fill({ color: hex(one ? color.border.strong : color.border.default), alpha: (one ? 0.7 : 0.35) * fade });
        }
      }
      // The hit line pulses on each beat.
      hitGlow.alpha = locked && sinceBeat >= 0 ? 0.35 + 0.65 * Math.exp(-sinceBeat / 160) : 0.35;

      notes.clear();
      glow.clear();
      const heard = listening ? liveChordWindow(now - behindMs, now + AHEAD_BEATS * beatMs, now) : [];
      const blocks = chordBlocks(locked ? snapToBeats(heard, clock.ms, beatMs) : heard);
      let usedNames = 0;
      let usedLetters = 0;

      /** One chord's keys from `top` to `bottom`, named at its lower end (as far as it shows). */
      const block = (name: string, keysOf: { left: number[]; right: number[] }, top: number, bottom: number, alpha: number, soon: boolean) => {
        top = Math.max(top, 0);
        if (bottom - top < 2) return;
        const foot = Math.min(bottom, height);
        let lowest: KeyRect | null = null;
        for (const [hand, list] of [['left', keysOf.left], ['right', keysOf.right]] as const) {
          const fill = hex(hand === 'left' ? color.note.melody : color.note.chord);
          for (const key of list) {
            const k = keys.get(key);
            if (!k) continue;
            if (!lowest || k.x < lowest.x) lowest = k;
            notes.roundRect(k.x + 1, top, k.w - 2, bottom - top, radius.sm).fill({ color: fill, alpha });
            if (soon) glow.roundRect(k.x + 1, Math.max(top, bottom - 24), k.w - 2, Math.min(24, bottom - top), radius.sm).fill({ color: fill, alpha: glowTokens.note.alpha });
            if (foot - top > 18 && k.w >= 12 && usedLetters < MAX_LABELS * 5) {
              const t = pooled(letters, usedLetters++, letterText);
              setText(t, NOTE_NAMES[key % 12]!);
              t.alpha = Math.min(1, alpha + 0.3);
              t.position.set(k.x + (k.w - t.width) / 2, foot - t.height - 3);
            }
          }
        }
        if (lowest && usedNames < MAX_LABELS && foot - top > 14) {
          const t = pooled(names, usedNames++, nameText);
          setText(t, name);
          t.alpha = Math.min(1, alpha + 0.3);
          // Left of the lowest key, or right of it at the keyboard's left edge.
          const x = lowest.x - t.width - 8;
          t.position.set(x >= 4 ? x : lowest.x + lowest.w + 6, Math.max(top, foot - 22));
        }
      };

      let ahead = false;
      for (const b of blocks) {
        const v = voicing(b.chord!, level as AlongLevel);
        const name = b.name ?? '';
        if (b.fromMs > now) ahead = true;
        // Coming and sounding chords are bright; the ones over fade as they move away.
        const alpha = b.toMs > now ? NOTE_ALPHA : 0.2 + 0.4 * Math.max(0, 1 - (now - b.toMs) / behindMs);
        block(name, v, y(b.toMs) + 1, y(b.fromMs) - 1, alpha, b.fromMs > now && b.fromMs - now < beatMs / 2);
      }
      for (let i = usedNames; i < names.children.length; i++) names.children[i]!.visible = false;
      for (let i = usedLetters; i < letters.children.length; i++) letters.children[i]!.visible = false;

      const hint = !listening ? 'Chords fall here as they are heard' : blocks.length === 0 ? 'Listening for chords…' : ahead ? '' : 'Chords coming fall here once the song repeats';
      setText(message, hint);
      message.visible = hint !== '';
      message.position.set((width - message.width) / 2, Math.max(8, (hitY - message.height) / 3));
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
        app.stage.addChild(lanes, bars, notes, glow, hitGlow, hitLine, letters, names, message);
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

  return <div ref={hostRef} className="note-lanes chord-lanes" role="img" aria-label="Falling chords: the keys to play" data-testid="chord-lanes" />;
}
