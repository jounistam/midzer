import { DRUM_PIECES } from '@midzer/audio-web';
import { describe, expect, it } from 'vitest';
import { KIT_PRESETS, kitCommands, kitMap, parseKitSetup, PIECE_INFO, PIECES, qwertyToPiece } from './kit';
import { layoutKit, pieceAt } from './kitLayout';

describe('kit', () => {
  it('gives every piece its own computer key', () => {
    const keys = PIECES.map((p) => PIECE_INFO[p].key);
    expect(new Set(keys).size).toBe(PIECES.length);
    expect(keys).not.toContain(' ');
    expect(qwertyToPiece('F')).toBe('snare');
    expect(qwertyToPiece('z')).toBeUndefined();
  });

  it('maps General MIDI like the engine, and Roland extras on top', () => {
    const gm = kitMap({ preset: 'gm' });
    for (const p of DRUM_PIECES) expect(gm[p.note]).toBe(p.id);
    expect(gm[26]).toBeUndefined();
    const roland = kitMap({ preset: 'roland' });
    expect(roland[26]).toBe('hiHatOpen');
    expect(roland[52]).toBe('crash2'); // a crash edge, not GM's china
    expect(KIT_PRESETS.map((p) => p.id)).toEqual(['gm', 'roland']);
  });

  it('loads a whole map into the engine, clearing other notes', () => {
    const cmds = kitCommands({ 38: 'snare' });
    expect(cmds).toHaveLength(128);
    expect(cmds[38]).toEqual({ type: 'kitNote', note: 38, piece: 'snare' });
    expect(cmds[36]).toEqual({ type: 'kitNote', note: 36, piece: null });
  });

  it('reads saved setups defensively', () => {
    expect(parseKitSetup(undefined)).toEqual({ preset: 'gm' });
    expect(parseKitSetup({ preset: 'roland' })).toEqual({ preset: 'roland' });
    expect(parseKitSetup({ preset: 'learned', map: { 38: 'snare', 300: 'kick', 40: 'cowbell' } })).toEqual({
      preset: 'learned',
      map: { 38: 'snare' },
    });
  });
});

describe('kit layout', () => {
  const pads = layoutKit(1200, 300);

  it('fits every piece inside the view', () => {
    expect(pads.map((p) => p.piece).sort()).toEqual([...PIECES].sort());
    for (const p of pads) {
      expect(p.x - p.r).toBeGreaterThanOrEqual(0);
      expect(p.x + p.r).toBeLessThanOrEqual(1200);
      expect(p.y - p.r).toBeGreaterThanOrEqual(0);
      expect(p.y + p.r).toBeLessThanOrEqual(300);
    }
  });

  it('only nests rings around their own pad', () => {
    const nested = new Set(['hiHatOpen:hiHatClosed', 'snareRim:snare', 'ride:rideBell']);
    for (const a of pads) {
      for (const b of pads) {
        if (a === b || Math.hypot(a.x - b.x, a.y - b.y) >= a.r + b.r) continue;
        expect(nested.has(`${a.piece}:${b.piece}`) || nested.has(`${b.piece}:${a.piece}`), `${a.piece} overlaps ${b.piece}`).toBe(true);
      }
    }
  });

  it('hits the smallest pad under the pointer', () => {
    const at = (piece: string) => pads.find((p) => p.piece === piece)!;
    const ride = at('ride');
    expect(pieceAt(pads, ride.x, ride.y)).toBe('rideBell');
    expect(pieceAt(pads, ride.x + ride.r * 0.8, ride.y)).toBe('ride');
    const snare = at('snare');
    expect(pieceAt(pads, snare.x, snare.y)).toBe('snare');
    expect(pieceAt(pads, 1, 1)).toBeNull();
  });
});
