// The stored songs the listener can recognise in a live source and follow:
// those with a fingerprint and a beat map (read from a recording, or learned
// live). Each gets a small number for the session, mapped back to its id.

import type { LiveLibrarySong } from '@midzer/audio-web';
import { isNoChord, parseChord } from './chords';
import type { Song } from './song';

/** The beat map the song's fingerprint lines up with. */
export function beatMapOf(song: Song): { beatTimes: number[]; seconds: number } | null {
  return song.backing ?? song.heard ?? null;
}

/** The chord of each chart beat as the listener takes it: root · 16 + quality, −1 for none. */
export function chordCodes(song: Song): Int32Array {
  const codes: number[] = [];
  let code = -1;
  for (const bar of song.bars) {
    for (const token of bar.beats) {
      if (token !== '') {
        const chord = isNoChord(token) ? null : parseChord(token);
        code = chord ? chord.root * 16 + chord.quality : -1;
      }
      codes.push(code);
    }
  }
  return Int32Array.from(codes);
}

/** The song for the listener, or null if it can't be recognised (no fingerprint or no beat map). */
export function liveLibrarySong(id: number, song: Song, landmarks: Uint32Array | null): LiveLibrarySong | null {
  const map = beatMapOf(song);
  if (!map || map.beatTimes.length < 2 || !landmarks || landmarks.length < 2) return null;
  const chords = chordCodes(song);
  return {
    id,
    landmarks,
    beatTimes: Float64Array.from(map.beatTimes),
    chords: chords.slice(0, Math.min(chords.length, map.beatTimes.length - 1)),
    seconds: map.seconds,
    barBeats: Uint8Array.from(song.bars, (bar) => bar.beats.length),
  };
}

/** A live song heard this much longer than the part kept is worth learning again. */
const MORE_S = 10;

/**
 * What to do with a song captured live, given the stored song followed in
 * it: learn it as new, drop it (it is kept already), or offer it as more of
 * a song kept only in part.
 */
export function captureFate(followed: Song | undefined, capture: { seconds: number; complete: boolean }): 'new' | 'known' | 'more' {
  if (!followed) return 'new';
  const heard = followed.heard;
  if (followed.source !== 'live' || !heard || heard.complete) return 'known';
  return capture.complete || capture.seconds >= heard.seconds + MORE_S ? 'more' : 'known';
}

/** Numbers songs for the listener, the same number for the same song all session. */
export class LiveIds {
  private ids = new Map<string, number>();
  private songs: string[] = [];

  of(songId: string): number {
    let id = this.ids.get(songId);
    if (id === undefined) {
      id = this.songs.length;
      this.ids.set(songId, id);
      this.songs.push(songId);
    }
    return id;
  }

  songOf(id: number): string | undefined {
    return this.songs[id];
  }
}
