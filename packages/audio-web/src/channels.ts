// MIDI channels used by the engine; mirrors crates/midzer-core (engine.rs, style.rs).

export const PLAYER_CHANNEL = 0;
export const KEYS_CHANNEL = 1;
export const BASS_CHANNEL = 2;
export const PAD_CHANNEL = 3;
/** The song's tune, played under the band in Learn; not shown on the keys. Mirrors `MELODY_CHANNEL` in melody.rs. */
export const MELODY_CHANNEL = 4;
export const DRUM_CHANNEL = 9;
/** The player's drum hits, one General MIDI key per piece. Mirrors `PLAYER_DRUM_CHANNEL` in event.rs. */
export const PLAYER_DRUM_CHANNEL = 14;
/** What the microphone heard; reported, never played. Mirrors `MIC_CHANNEL` in event.rs. */
export const MIC_CHANNEL = 15;

export type BandPart = 'keys' | 'bass' | 'pad' | 'drums';

export function partOfChannel(channel: number): BandPart | 'player' | 'playerDrums' | null {
  switch (channel) {
    case PLAYER_CHANNEL:
      return 'player';
    case PLAYER_DRUM_CHANNEL:
      return 'playerDrums';
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
