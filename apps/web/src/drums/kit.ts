// The drummer's kit on the UI side: names, computer-keyboard pads, note maps
// for common e-kits, and the commands that load a map into the engine.

import { DRUM_PIECES, KIT_ACTIONS, type DrumPiece, type EngineCommand, type KitAction, type KitTrigger } from '@midzer/audio-web';

export interface PieceInfo {
  label: string;
  /** Fits on a pad. */
  short: string;
  /** The computer key that hits it. */
  key: string;
}

export const PIECE_INFO: Record<DrumPiece, PieceInfo> = {
  kick: { label: 'Kick', short: 'Kick', key: 'v' },
  snare: { label: 'Snare', short: 'Snare', key: 'f' },
  snareRim: { label: 'Snare rim', short: 'Rim', key: 'r' },
  sideStick: { label: 'Cross-stick', short: 'Stick', key: 'd' },
  hiHatClosed: { label: 'Hi-hat closed', short: 'HH', key: 'j' },
  hiHatOpen: { label: 'Hi-hat open', short: 'Open', key: 'u' },
  hiHatPedal: { label: 'Hi-hat pedal', short: 'Pedal', key: 'n' },
  tom1: { label: 'Tom 1', short: 'Tom 1', key: 't' },
  tom2: { label: 'Tom 2', short: 'Tom 2', key: 'y' },
  tom3: { label: 'Floor tom', short: 'Floor', key: 'h' },
  tom4: { label: 'Low floor tom', short: 'Low', key: 'b' },
  crash: { label: 'Crash', short: 'Crash', key: 'e' },
  crash2: { label: 'Crash 2', short: 'Crash 2', key: 'i' },
  ride: { label: 'Ride', short: 'Ride', key: 'k' },
  rideBell: { label: 'Ride bell', short: 'Bell', key: 'o' },
  splash: { label: 'Splash', short: 'Splash', key: 'w' },
  china: { label: 'China', short: 'China', key: 'p' },
};

export const PIECES: readonly DrumPiece[] = DRUM_PIECES.map((p) => p.id);

const KEY_TO_PIECE = new Map(PIECES.map((p) => [PIECE_INFO[p].key, p]));

export function qwertyToPiece(key: string): DrumPiece | undefined {
  return KEY_TO_PIECE.get(key.toLowerCase());
}

/** Which piece each note plays; notes not listed are ignored. */
export type KitMap = Partial<Record<number, DrumPiece>>;

/** General MIDI, as the engine starts out (`Kit::default` in drums.rs). */
const GM: KitMap = {
  ...Object.fromEntries(DRUM_PIECES.map((p) => [p.note, p.id])),
  35: 'kick',
  50: 'tom1',
  47: 'tom2',
  59: 'ride',
};

/**
 * Roland V-Drums' default note map: extra notes for the rims of the toms and
 * the edges of the hi-hat, crashes and ride.
 */
const ROLAND: KitMap = {
  ...GM,
  22: 'hiHatClosed',
  26: 'hiHatOpen',
  58: 'tom3',
  39: 'tom4',
  55: 'crash',
  52: 'crash2',
  59: 'ride',
};

export type KitPresetId = 'gm' | 'roland';

export const KIT_PRESETS: { id: KitPresetId; label: string; map: KitMap }[] = [
  // Alesis, Yamaha DTX and most other kits send General MIDI notes by default.
  { id: 'gm', label: 'General MIDI', map: GM },
  { id: 'roland', label: 'Roland V-Drums', map: ROLAND },
];

/** A preset, or a map learned from the player's own kit. */
export type KitSetup = { preset: KitPresetId } | { preset: 'learned'; map: KitMap };

export function kitMap(setup: KitSetup): KitMap {
  if (setup.preset === 'learned') return setup.map;
  return KIT_PRESETS.find((p) => p.id === setup.preset)?.map ?? GM;
}

/** Loads a map into the engine: one command per note, so old mappings go. */
export function kitCommands(map: KitMap): EngineCommand[] {
  return Array.from({ length: 128 }, (_, note) => ({ type: 'kitNote', note, piece: map[note] ?? null }) as const);
}

/** Reads a saved setup, falling back to General MIDI. */
export function parseKitSetup(value: unknown): KitSetup {
  const v = value as { preset?: unknown; map?: unknown } | null;
  if (v?.preset === 'learned' && v.map && typeof v.map === 'object') {
    const map: KitMap = {};
    for (const [note, piece] of Object.entries(v.map)) {
      const n = Number(note);
      if (Number.isInteger(n) && n >= 0 && n < 128 && PIECES.includes(piece as DrumPiece)) map[n] = piece as DrumPiece;
    }
    return { preset: 'learned', map };
  }
  if (KIT_PRESETS.some((p) => p.id === v?.preset)) return { preset: v!.preset as KitPresetId };
  return { preset: 'gm' };
}

// ---- Kit controls -------------------------------------------------------------

export const KIT_ACTION_INFO: Record<KitAction, { label: string; hint: string }> = {
  nextSection: { label: 'Next section', hint: 'A → B → C, with a fill into it' },
  fill: { label: 'Fill', hint: 'The band fills to the end of the bar' },
  ending: { label: 'Ending', hint: 'The band ends on the next bar' },
  startStop: { label: 'Start / stop', hint: 'Starts the band at once, or stops it' },
};

/** Which pad or pedal sets off each action. */
export type KitTriggers = Partial<Record<KitAction, KitTrigger>>;

export function triggerLabel(trigger: KitTrigger): string {
  return trigger.kind === 'note' ? `Pad · note ${trigger.number}` : `Pedal or switch · CC ${trigger.number}`;
}

const sameTrigger = (a: KitTrigger | undefined, b: KitTrigger) => a?.kind === b.kind && a.number === b.number;

/**
 * Assigns `trigger` to `action` (null clears it). A trigger does one thing,
 * so another action using it loses it. Returns the new assignments and the
 * engine commands that make the change.
 */
export function assignTrigger(triggers: KitTriggers, action: KitAction, trigger: KitTrigger | null): { triggers: KitTriggers; commands: EngineCommand[] } {
  const next: KitTriggers = { ...triggers };
  const commands: EngineCommand[] = [];
  const old = next[action];
  if (old) commands.push({ type: 'kitTrigger', trigger: old, action: null });
  delete next[action];
  if (trigger) {
    for (const other of KIT_ACTIONS) if (sameTrigger(next[other], trigger)) delete next[other];
    next[action] = trigger;
    commands.push({ type: 'kitTrigger', trigger, action });
  }
  return { triggers: next, commands };
}

export function triggerCommands(triggers: KitTriggers): EngineCommand[] {
  return KIT_ACTIONS.flatMap((action) => {
    const trigger = triggers[action];
    return trigger ? [{ type: 'kitTrigger', trigger, action } as const] : [];
  });
}

export function parseKitTriggers(value: unknown): KitTriggers {
  const out: KitTriggers = {};
  if (!value || typeof value !== 'object') return out;
  for (const action of KIT_ACTIONS) {
    const t = (value as Record<string, { kind?: unknown; number?: unknown } | undefined>)[action];
    const n = Number(t?.number);
    if ((t?.kind === 'note' || t?.kind === 'control') && Number.isInteger(n) && n >= 0 && n < 128) out[action] = { kind: t.kind, number: n };
  }
  return out;
}
