import { describe, expect, it } from 'vitest';
import { envelopeOf } from './envelope';

describe('envelope', () => {
  it('reads the level 100 times a second, −60 to 0 dBFS as 0 to 255', () => {
    const samples = new Int16Array(22_050);
    // The second half a sine at full scale: −3 dBFS.
    for (let i = 11_025; i < samples.length; i++) samples[i] = Math.round(32767 * Math.sin(i / 3));
    const env = envelopeOf(samples);
    expect(env.length).toBe(100);
    expect(env[10]).toBe(0);
    expect(env[80]).toBeGreaterThan(240);
    expect(env[80]).toBeLessThan(246);
  });
});
