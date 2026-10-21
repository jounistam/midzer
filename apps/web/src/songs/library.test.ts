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
});
