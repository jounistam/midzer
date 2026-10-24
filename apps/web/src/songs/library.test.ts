import { beforeEach, describe, expect, it, vi } from 'vitest';
import type { Song } from './song';

const song = (id: string, title = id): Song => ({ id, title, bpm: 100, style: 0, bars: [{ beats: ['C', '', '', ''] }], source: 'user' });

describe('my songs', () => {
  beforeEach(() => {
    vi.resetModules();
    const data = new Map<string, string>();
    vi.stubGlobal('localStorage', {
      getItem: (k: string) => data.get(k) ?? null,
      setItem: (k: string, v: string) => void data.set(k, v),
      removeItem: (k: string) => void data.delete(k),
    });
  });

  it('moves songs from localStorage once, keeping their order', async () => {
    localStorage.setItem('midzer.songs', JSON.stringify([song('new'), song('old'), { broken: true }]));
    const lib = await import('./library');
    expect((await lib.loadUserSongs()).map((s) => s.id)).toEqual(['new', 'old']);
    expect(localStorage.getItem('midzer.songs')).toBeNull();
  });

  it('saves, replaces, lists newest first and deletes, melody included', async () => {
    const lib = await import('./library');
    expect(await lib.saveUserSong(song('a'))).toBe(true);
    await new Promise((r) => setTimeout(r, 2));
    await lib.saveUserSong({ ...song('b'), melody: [{ at: 0, len: 1, key: 60 }, { at: -1, len: 1, key: 60 } as never] });
    await lib.saveUserSong(song('a', 'renamed'));
    const list = await lib.loadUserSongs();
    expect(list.map((s) => s.title)).toEqual(['b', 'renamed']);
    expect(list[0]!.melody).toEqual([{ at: 0, len: 1, key: 60 }]);
    await lib.deleteUserSong('b');
    expect((await lib.loadUserSongs()).map((s) => s.id)).toEqual(['a']);
  });

  it('keeps a recording with its song and its tempo map, and deletes them together', async () => {
    const lib = await import('./library');
    const recordings = await import('./recordings');
    const backing = { beatTimes: [0.5, 1, 1.5, 2, 2.5], seconds: 30, stemsDir: '/stems/1' };
    await lib.saveUserSong({ ...song('rec'), source: 'audio', backing });
    await lib.saveUserSong({ ...song('bad'), backing: { beatTimes: [1, 0.5], seconds: 30 } });
    await recordings.saveRecording('rec', new Uint8Array([1, 2, 3]));
    const list = await lib.loadUserSongs();
    expect(list.find((s) => s.id === 'rec')!.backing).toEqual(backing);
    expect(list.find((s) => s.id === 'bad')!.backing).toBeUndefined();
    expect(await recordings.loadRecording('rec')).toEqual(new Uint8Array([1, 2, 3]));
    await lib.deleteUserSong('rec');
    expect(await recordings.loadRecording('rec')).toBeNull();
  });

  it('keeps a song heard live with its fingerprint, and deletes them together', async () => {
    const lib = await import('./library');
    const prints = await import('./fingerprints');
    const heard = { beatTimes: [0.5, 1, 1.5], seconds: 40, complete: true, envelope: new Uint8Array([1, 2]), heardAt: 5 };
    await lib.saveUserSong({ ...song('live'), source: 'live', heard });
    await lib.saveUserSong({ ...song('odd'), source: 'live', heard: { ...heard, beatTimes: [2, 1], envelope: [1] as unknown as Uint8Array } });
    await prints.saveFingerprint('live', new Uint32Array([1, 0, 2, 4]));
    const list = await lib.loadUserSongs();
    expect(list.find((s) => s.id === 'live')).toMatchObject({ source: 'live', heard });
    expect(list.find((s) => s.id === 'odd')!.heard).toBeUndefined();
    expect(await prints.loadFingerprint('live')).toEqual(new Uint32Array([1, 0, 2, 4]));
    await lib.deleteUserSong('live');
    expect(await prints.loadFingerprint('live')).toBeNull();
  });
});
