// Web MIDI input and output. The message translation is kept pure so it can
// be tested without a browser.

import type { EngineCommand, EngineEvent } from './protocol';
import { MIC_CHANNEL, PLAYER_CHANNEL } from './channels';

/** Translates a raw MIDI message from a controller into an engine command. */
export function midiToCommand(data: ArrayLike<number>): EngineCommand | null {
  const status = data[0] ?? 0;
  const kind = status & 0xf0;
  const d1 = data[1] ?? 0;
  const d2 = data[2] ?? 0;
  if (kind === 0x90 && d2 > 0) return { type: 'noteOn', note: d1, velocity: d2 };
  if (kind === 0x80 || (kind === 0x90 && d2 === 0)) return { type: 'noteOff', note: d1 };
  // All Sound Off / All Notes Off
  if (kind === 0xb0 && (d1 === 120 || d1 === 123)) return { type: 'allNotesOff' };
  return null;
}

/** Translates a band event into MIDI bytes for an external synth or DAW. */
export function eventToMidi(event: EngineEvent, { echoPlayer = false } = {}): number[] | null {
  switch (event.type) {
    case 'noteOn':
      if (event.channel === MIC_CHANNEL || (event.channel === PLAYER_CHANNEL && !echoPlayer)) return null;
      return [0x90 | event.channel, event.key, event.velocity];
    case 'noteOff':
      if (event.channel === MIC_CHANNEL || (event.channel === PLAYER_CHANNEL && !echoPlayer)) return null;
      return [0x80 | event.channel, event.key, 0];
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
