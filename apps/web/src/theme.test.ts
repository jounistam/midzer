import { readFileSync } from 'node:fs';
import { describe, expect, it } from 'vitest';
import { cssVariables } from './theme';

describe('theme', () => {
  const vars = new Map(cssVariables());

  it('uses the Figma variable names', () => {
    expect(vars.get('--color-bg-stage')).toBe('#0a0912');
    expect(vars.get('--color-text-on-primary')).toBe('#1a0010');
    expect(vars.get('--color-key-band-white')).toBe('#a99fdb');
    expect(vars.get('--space-12')).toBe('48px');
    expect(vars.get('--radius-control')).toBe('10px');
    expect(vars.get('--motion-duration-beat-pulse')).toBe('180ms');
  });

  it('defines every variable the stylesheet uses', () => {
    const css = readFileSync(new URL('./index.css', import.meta.url), 'utf8');
    const used = new Set([...css.matchAll(/var\((--[a-z0-9-]+)\)/g)].map((m) => m[1]!));
    expect([...used].filter((v) => !vars.has(v))).toEqual([]);
  });
});
