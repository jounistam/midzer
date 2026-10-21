// Web MIDI input and output. The message translation is kept pure so it can
// be tested without a browser.

import type { EngineCommand, EngineEvent } from './protocol';
import { DRUM_CHANNEL, MIC_CHANNEL, PLAYER_CHANNEL, PLAYER_DRUM_CHANNEL } from './channels';

/** The MIDI controller for the hi-hat pedal (foot controller). */
const HI_HAT_CC = 4;

/**
 * Translates a raw MIDI message from a controller into an engine command.
 * Channel 10, where e-kits send, plays drums; mirrors `Command::from_midi`.
 */
export function midiToCommand(data: ArrayLike<number>): EngineCommand | null {
  const status = data[0] ?? 0;
  const kind = status & 0xf0;
  const drums = (status & 0x0f) === DRUM_CHANNEL;
  const d1 = data[1] ?? 0;
  const d2 = data[2] ?? 0;
  if (kind === 0x90 && d2 > 0) return drums ? { type: 'drumNote', note: d1, velocity: d2 } : { type: 'noteOn', note: d1, velocity: d2 };
  // Drum hits don't hold: their releases mean nothing.
  if (kind === 0x80 || kind === 0x90) return drums ? null : { type: 'noteOff', note: d1 };
  if (kind === 0xa0 && d2 > 0) return { type: 'drumChoke', note: d1 };
  if (kind === 0xb0 && d1 === HI_HAT_CC) return { type: 'hiHatPedal', value: d2 };
  // All Sound Off / All Notes Off
  if (kind === 0xb0 && (d1 === 120 || d1 === 123)) return { type: 'allNotesOff' };
  return null;
}

/**
 * Translates a band event into MIDI bytes for an external synth or DAW.
 * Echoed player drums go out on the drum channel.
 */
export function eventToMidi(event: EngineEvent, { echoPlayer = false } = {}): number[] | null {
  switch (event.type) {
    case 'noteOn':
    case 'noteOff': {
      const player = event.channel === PLAYER_CHANNEL || event.channel === PLAYER_DRUM_CHANNEL;
      if (event.channel === MIC_CHANNEL || (player && !echoPlayer)) return null;
      const channel = event.channel === PLAYER_DRUM_CHANNEL ? DRUM_CHANNEL : event.channel;
      return event.type === 'noteOn' ? [0x90 | channel, event.key, event.velocity] : [0x80 | channel, event.key, 0];
    }
    case 'programChange':
      return [0xc0 | event.channel, event.program];
    default:
      return null;
  }
}

export function isMidiSupported(): boolean {
  return typeof navigator !== 'undefined' && 'requestMIDIAccess' in navigator;
}

/** Resolves null when Web MIDI is unsupported (e.g. Safari) or permission is denied. */
export async function requestMidi(): Promise<MIDIAccess | null> {
  if (!isMidiSupported()) return null;
  try {
    return await navigator.requestMIDIAccess({ sysex: false });
  } catch {
    return null;
  }
}

/**
 * Feeds every connected MIDI input into `onCommand`, including devices
 * plugged in later. Returns a function that disconnects them all.
 */
export function listenToInputs(access: MIDIAccess, onCommand: (cmd: EngineCommand) => void): () => void {
  const handler = (e: MIDIMessageEvent) => {
    if (!e.data) return;
    const cmd = midiToCommand(e.data);
    if (cmd) onCommand(cmd);
  };
  const attach = () => access.inputs.forEach((input) => (input.onmidimessage = handler));
  attach();
  const onStateChange = () => attach();
  access.addEventListener('statechange', onStateChange);
  return () => {
    access.removeEventListener('statechange', onStateChange);
    access.inputs.forEach((input) => (input.onmidimessage = null));
  };
}
