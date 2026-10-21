// The recordings songs were read from, kept as the original file so the song
// can play along with it (see SongBacking). They stay on this device.

import { store } from '../storage/db';

const recordings = store<Uint8Array>('recordings');
/** The last one saved, so a song opened right after its import doesn't wait for storage. */
let recent: { id: string; bytes: Uint8Array } | null = null;

/** Resolves false if it could not be stored (the song still plays it until closed). */
export async function saveRecording(songId: string, bytes: Uint8Array): Promise<boolean> {
  recent = { id: songId, bytes };
  try {
    await recordings.put(songId, bytes);
    return true;
  } catch {
    return false;
  }
}

export async function loadRecording(songId: string): Promise<Uint8Array | null> {
  if (recent?.id === songId) return recent.bytes;
  try {
    return (await recordings.get(songId)) ?? null;
  } catch {
    return null;
  }
}

export async function deleteRecording(songId: string): Promise<void> {
  if (recent?.id === songId) recent = null;
  try {
    await recordings.delete(songId);
  } catch {
    // Left behind; it is only found through its song.
  }
}
