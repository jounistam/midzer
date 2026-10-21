// Persistent key-value stores in IndexedDB: the player's songs and their Learn
// progress. Falls back to memory when IndexedDB is missing or blocked (some
// private modes), so the app still works, just without keeping anything.

export type StoreName = 'songs' | 'progress';

export interface Store<T> {
  get(key: string): Promise<T | undefined>;
  put(key: string, value: T): Promise<void>;
  delete(key: string): Promise<void>;
  all(): Promise<T[]>;
}

const DB_NAME = 'midzer';
const DB_VERSION = 1;
const STORES: StoreName[] = ['songs', 'progress'];

let opening: Promise<IDBDatabase | null> | null = null;

function open(): Promise<IDBDatabase | null> {
  opening ??= new Promise((resolve) => {
    try {
      const req = indexedDB.open(DB_NAME, DB_VERSION);
      req.onupgradeneeded = () => {
        for (const name of STORES) if (!req.result.objectStoreNames.contains(name)) req.result.createObjectStore(name);
      };
      req.onsuccess = () => resolve(req.result);
      req.onerror = () => resolve(null);
      req.onblocked = () => resolve(null);
    } catch {
      resolve(null);
    }
  });
  return opening;
}

const done = <R>(req: IDBRequest<R>) =>
  new Promise<R>((resolve, reject) => {
    req.onsuccess = () => resolve(req.result);
    req.onerror = () => reject(req.error ?? new Error('IndexedDB request failed'));
  });

function memoryStore<T>(): Store<T> {
  const map = new Map<string, T>();
  return {
    get: async (key) => structuredClone(map.get(key)),
    put: async (key, value) => void map.set(key, structuredClone(value)),
    delete: async (key) => void map.delete(key),
    all: async () => [...map.values()].map((v) => structuredClone(v)),
  };
}

const memory = new Map<StoreName, Store<unknown>>();

/** A store backed by IndexedDB, or by memory when that isn't available. */
export function store<T>(name: StoreName): Store<T> {
  const fallback = () => {
    if (!memory.has(name)) memory.set(name, memoryStore());
    return memory.get(name) as Store<T>;
  };
  const run = async <R>(mode: IDBTransactionMode, fn: (s: IDBObjectStore) => IDBRequest<R>, orElse: (m: Store<T>) => Promise<R>) => {
    const db = typeof indexedDB === 'undefined' ? null : await open();
    if (!db) return orElse(fallback());
    return done(fn(db.transaction(name, mode).objectStore(name)));
  };
  return {
    get: (key) => run('readonly', (s) => s.get(key) as IDBRequest<T | undefined>, (m) => m.get(key)),
    put: async (key, value) => void (await run('readwrite', (s) => s.put(value, key), (m) => m.put(key, value).then(() => key))),
    delete: async (key) => void (await run('readwrite', (s) => s.delete(key), (m) => m.delete(key).then(() => undefined))),
    all: () => run('readonly', (s) => s.getAll() as IDBRequest<T[]>, (m) => m.all()),
  };
}
