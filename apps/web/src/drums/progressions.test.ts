import { STYLES } from '@midzer/audio-web';
import { describe, expect, it } from 'vitest';
import { parseChart } from '../songs/song';
import { pickProgression, PROGRESSIONS, progressionCommands, progressionsFor } from './progressions';

describe('progressions', () => {
  it('gives every style a few clean loops', () => {
    expect(PROGRESSIONS).toHaveLength(STYLES.length);
    const ids = new Set<string>();
    for (const list of PROGRESSIONS) {
      expect(list.length).toBeGreaterThanOrEqual(2);
      for (const p of list) {
        const { bars, unknown } = parseChart(p.chart);
        expect(unknown, p.id).toEqual([]);
        expect([4, 8, 12], p.id).toContain(bars.length);
        expect(bars.every((b) => !b.section && !b.fill), 'no marks: the drummer has the sections').toBe(true);
        expect(ids.has(p.id)).toBe(false);
        ids.add(p.id);
      }
    }
  });

  it('keeps a progression the style has, else takes its first', () => {
    expect(pickProgression(1, 'rock-three').id).toBe('rock-three');
    expect(pickProgression(1, 'pop-sad').id).toBe(progressionsFor(1)[0]!.id);
    expect(pickProgression(99, null).id).toBe('pop-anthem');
  });

  it('loads as a chart that loops all of it', () => {
    const cmds = progressionCommands(pickProgression(0, 'pop-anthem'));
    expect(cmds[0]).toEqual({ type: 'songClear', bars: 4 });
    expect(cmds.filter((c) => c.type === 'songChord')).toHaveLength(4);
    expect(cmds.at(-1)).toEqual({ type: 'songLoop', start: 0, end: 4 });
  });
});
