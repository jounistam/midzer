import { describe, expect, it } from 'vitest';
import { RecordRing } from './ring';

describe('RecordRing', () => {
  it('delivers records in order', () => {
    const ring = RecordRing.create(4);
    const out = new Uint32Array(4);
    expect(ring.push([1, 60, 100, 0])).toBe(true);
    expect(ring.push([2, 60, 0, 0])).toBe(true);
    expect(ring.pop(out)).toBe(true);
    expect([...out]).toEqual([1, 60, 100, 0]);
    expect(ring.pop(out)).toBe(true);
    expect([...out]).toEqual([2, 60, 0, 0]);
    expect(ring.pop(out)).toBe(false);
  });

  it('rejects pushes when full', () => {
    const ring = RecordRing.create(2);
    expect(ring.push([1, 0, 0, 0])).toBe(true);
    expect(ring.push([1, 0, 0, 0])).toBe(true);
    expect(ring.push([1, 0, 0, 0])).toBe(false);
  });

  it('keeps working across many wraps', () => {
    const ring = RecordRing.create(8);
    const out = new Uint32Array(4);
    for (let i = 0; i < 10_000; i++) {
      expect(ring.push([i, i + 1, i + 2, i + 3])).toBe(true);
      expect(ring.pop(out)).toBe(true);
      expect(out[0]).toBe(i);
    }
  });

  it('can be reopened from the same buffer on the other thread', () => {
    const producer = RecordRing.create(4);
    const consumer = new RecordRing(producer.buffer);
    producer.push([3, 7, 0, 0]);
    const out = new Uint32Array(4);
    expect(consumer.pop(out)).toBe(true);
    expect(out[1]).toBe(7);
  });

  it('requires a power-of-two capacity', () => {
    expect(() => RecordRing.create(3)).toThrow(RangeError);
  });
});
