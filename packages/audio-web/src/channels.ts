// MIDI channels used by the engine; mirrors crates/midzer-core (engine.rs, style.rs).

export const PLAYER_CHANNEL = 0;
export const KEYS_CHANNEL = 1;
export const BASS_CHANNEL = 2;
export const PAD_CHANNEL = 3;
export const DRUM_CHANNEL = 9;

export type BandPart = 'keys' | 'bass' | 'pad' | 'drums';

export function partOfChannel(channel: number): BandPart | 'player' | null {
  switch (channel) {
    case PLAYER_CHANNEL:
      return 'player';
    case KEYS_CHANNEL:
      return 'keys';
    case BASS_CHANNEL:
      return 'bass';
    case PAD_CHANNEL:
      return 'pad';
    case DRUM_CHANNEL:
      return 'drums';
    default:
      return null;
  }
}
