// Design tokens for direction A "Stage / club": a mirror of the "midzer tokens"
// variable collection in the D0 Figma file. CSS custom property names match the
// variables' web code syntax (`color/bg/stage` → `--color-bg-stage`). This is the
// single source for both CSS (see applyTheme) and PixiJS (via `hex`).

export const color = {
  bg: { canvas: '#05040a', stage: '#0a0912' },
  surface: { default: '#14121f', raised: '#1f1c2e', hover: '#2a2640', track: '#2a2640' },
  border: { default: '#2e2a44', strong: '#4a4566' },
  text: {
    primary: '#f5f2ff',
    secondary: '#c9c3e3',
    muted: '#9c95bd',
    disabled: '#6f6890',
    onPrimary: '#1a0010',
    onAccent: '#0a0912',
    /** Magenta for text; #ff2e88 fails AA as text on raised surfaces. */
    brand: '#ff5c9f',
    accent: '#21e6ff',
  },
  primary: { default: '#ff2e88', hover: '#ff5c9f', pressed: '#e8287c' },
  accent: { default: '#21e6ff' },
  status: { success: '#3dffa2', warning: '#ffb020', danger: '#ff5c6c' },
  note: { chord: '#21e6ff', melody: '#ff2e88' },
  key: {
    white: '#d9d5ea',
    black: '#0e0d16',
    label: '#5c5677',
    bandWhite: '#a99fdb',
    bandBlack: '#4d4573',
    split: '#ff2e88',
  },
  focusRing: '#21e6ff',
  thumb: '#ffffff',
} as const;

export const space = { 0: 0, 1: 4, 2: 8, 3: 12, 4: 16, 5: 20, 6: 24, 8: 32, 10: 40, 12: 48, 16: 64 } as const;
export const radius = { key: 4, sm: 6, control: 10, card: 16, pill: 999 } as const;
export const size = { touchMin: 44, controlLg: 56, icon: 20 } as const;

export const font = {
  family: {
    display: "'Space Grotesk Variable', 'Space Grotesk', system-ui, sans-serif",
    body: "'Inter Variable', 'Inter', system-ui, sans-serif",
  },
} as const;

export const motion = {
  duration: { fast: 120, normal: 200, slow: 320, beatPulse: 180 },
  easing: { standard: 'cubic-bezier(0.2, 0, 0, 1)', pulse: 'cubic-bezier(0.1, 0.9, 0.2, 1)' },
} as const;

/** Glow effects (offset 0). PixiJS draws these as blurred shapes; CSS uses shadows. */
export const glow = {
  primaryButton: { blur: 24, color: color.primary.default, alpha: 0.55 },
  primarySmall: { blur: 12, color: color.primary.default, alpha: 0.8 },
  chordText: { blur: 36, color: color.accent.default, alpha: 0.85 },
  heldKey: { blur: 20, alpha: 0.9 },
  note: { blur: 12, alpha: 0.8 },
  beatPulse: { blur: 16, color: color.primary.default, alpha: 1, spread: 2 },
  splitLine: { blur: 10, color: color.key.split, alpha: 1 },
} as const;

/** Notes are drawn at this alpha over keys and lanes. */
export const NOTE_ALPHA = 0.85;

/** Converts '#rrggbb' to the 0xrrggbb form PixiJS takes. */
export const hex = (css: string): number => parseInt(css.slice(1), 16);

const kebab = (s: string) => s.replace(/[A-Z]/g, (c) => `-${c.toLowerCase()}`);
const px = (v: number) => (v === 0 ? '0' : `${v}px`);

/** Flattens nested tokens into `--prefix-path-to-token` properties. */
function flatten(prefix: string, tokens: object, format: (v: never) => string, out: [string, string][]) {
  for (const [k, v] of Object.entries(tokens)) {
    const name = `${prefix}-${kebab(k)}`;
    if (typeof v === 'object') flatten(name, v, format, out);
    else out.push([name, format(v as never)]);
  }
  return out;
}

export function cssVariables(): [string, string][] {
  const out: [string, string][] = [];
  flatten('--color', color, (v: string) => v, out);
  flatten('--space', space, px, out);
  flatten('--radius', radius, px, out);
  flatten('--size', size, px, out);
  flatten('--font', font, (v: string) => v, out);
  flatten('--motion-duration', motion.duration, (v: number) => `${v}ms`, out);
  flatten('--motion-easing', motion.easing, (v: string) => v, out);
  return out;
}

export function applyTheme(root: HTMLElement = document.documentElement): void {
  for (const [name, value] of cssVariables()) root.style.setProperty(name, value);
}
