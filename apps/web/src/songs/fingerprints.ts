// Each song's audio fingerprint (landmark hashes from crates/midzer-hear's
// fingerprint.rs: hash, frame, hash, frame, …), by song id, to recognise the
// song when it plays again. Kept for songs read from a recording and songs
// learned live, which keep no audio to make one from later.

import { store } from '../storage/db';

const fingerprints = store<Uint32Array>('fingerprints');

/** Resolves false if it could not be stored. */
export async function saveFingerprint(songId: string, landmarks: Uint32Array): Promise<boolean> {
  if (landmarks.length === 0) return true;
  try {
    await fingerprints.put(songId, landmarks);
    return true;
  } catch {
    return false;
  }
}

export async function loadFingerprint(songId: string): Promise<Uint32Array | null> {
  try {
    return (await fingerprints.get(songId)) ?? null;
  } catch {
    return null;
  }
}

export async function deleteFingerprint(songId: string): Promise<void> {
  try {
    await fingerprints.delete(songId);
  } catch {
    // Left behind; it is only found through its song.
  }
}
