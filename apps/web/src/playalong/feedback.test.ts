import { describe, expect, it } from 'vitest';
import { gradeOf, missOf, RecentHits } from './feedback';

describe('play-along feedback', () => {
  it('ticks only for hits clearly off, or a wrong chord', () => {
    expect(missOf(12, null)).toBeNull();
    expect(missOf(-29, undefined)).toBeNull();
    expect(missOf(-31, null)).toBe('early');
    expect(missOf(45, true)).toBe('late');
    expect(missOf(2, false)).toBe('chord');
  });

  it('grades hits for the display', () => {
    expect(gradeOf(-15)).toBe('on');
    expect(gradeOf(20)).toBe('near');
    expect(gradeOf(31)).toBe('off');
    expect(gradeOf(0, false)).toBe('off');
  });

  it('keeps the latest hits and the share on time', () => {
    const hits = new RecentHits();
    expect(hits.onTime()).toBeNull();
    for (let i = 0; i < 40; i++) hits.push(i < 30 ? 50 : 0, null, i);
    expect(hits.hits).toHaveLength(32);
    expect(hits.count).toBe(40);
    expect(hits.onTime(16)).toBeCloseTo(10 / 16);
    hits.push(0, false);
    expect(hits.onTime(1)).toBe(0);
    hits.reset();
    expect(hits.count).toBe(0);
  });
});
