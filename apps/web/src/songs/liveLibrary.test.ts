import { describe, expect, it } from 'vitest';
import { parseChord } from './chords';
import { captureFate, chordCodes, LiveIds, liveLibrarySong } from './liveLibrary';
import type { Song } from './song';

const song = (extra: Partial<Song> = {}): Song => ({
  id: 's1',
  title: 'Heard',
  bpm: 120,
  style: 0,
  bars: [{ beats: ['Am', '', 'G7', ''] }, { beats: ['N.C.', '', '', 'C'] }],
  source: 'live',
  ...extra,
});

const heard = (seconds: number, complete: boolean) => ({ beatTimes: [0, 0.5, 1, 1.5, 2, 2.5, 3, 3.5, 4], seconds, complete, envelope: new Uint8Array(), heardAt: 0 });

describe('the songs the listener recognises', () => {
  it('gives each beat its chord, held over the empty beats', () => {
    const code = (name: string) => {
      const c = parseChord(name)!;
      return c.root * 16 + c.quality;
    };
    expect([...chordCodes(song())]).toEqual([code('Am'), code('Am'), code('G7'), code('G7'), -1, -1, -1, code('C')]);
  });

  it('takes songs with a fingerprint and a beat map, as many chords as beats', () => {
    const marks = new Uint32Array([1, 2]);
    const entry = liveLibrarySong(3, song({ heard: heard(4, true) }), marks)!;
    expect(entry.id).toBe(3);
    expect(entry.beatTimes).toHaveLength(9);
    expect(entry.chords).toHaveLength(8);
    expect(entry.seconds).toBe(4);
    const imported = liveLibrarySong(1, song({ source: 'audio', backing: { beatTimes: [0, 1, 2], seconds: 9 } }), marks)!;
    expect(imported.chords).toHaveLength(2);
    expect(liveLibrarySong(1, song(), marks)).toBeNull();
    expect(liveLibrarySong(1, song({ heard: heard(4, true) }), null)).toBeNull();
  });

  it('learns a song again only when more of a part kept was heard', () => {
    const capture = (seconds: number, complete = false) => ({ seconds, complete });
    expect(captureFate(undefined, capture(40))).toBe('new');
    expect(captureFate(song({ source: 'audio', backing: { beatTimes: [0, 1], seconds: 200 } }), capture(300, true))).toBe('known');
    expect(captureFate(song({ heard: heard(200, true) }), capture(220, true))).toBe('known');
    expect(captureFate(song({ heard: heard(80, false) }), capture(85))).toBe('known');
    expect(captureFate(song({ heard: heard(80, false) }), capture(95))).toBe('more');
    expect(captureFate(song({ heard: heard(80, false) }), capture(60, true))).toBe('more');
  });

  it('numbers songs the same all session', () => {
    const ids = new LiveIds();
    expect([ids.of('a'), ids.of('b'), ids.of('a')]).toEqual([0, 1, 0]);
    expect(ids.songOf(1)).toBe('b');
    expect(ids.songOf(5)).toBeUndefined();
  });
});
