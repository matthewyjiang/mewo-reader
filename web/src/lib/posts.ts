import type { FeedPost } from './models';

/** Posts for one book, scoped so two accounts on one origin do not share a feed. */
export function postKey(server: string, username: string, bookId: string): string {
  return `${server}\n${username}\n${bookId}`;
}

export function postPrefix(server: string, username: string): string {
  return `${server}\n${username}\n`;
}

export interface PostCache {
  read(key: string): Promise<FeedPost[] | null>;
  write(key: string, posts: FeedPost[]): Promise<void>;
  delete(key: string): Promise<void>;
  clearPrefix(prefix: string): Promise<void>;
}

function isPost(value: unknown): value is FeedPost {
  if (!value || typeof value !== 'object') return false;
  const post = value as Partial<FeedPost>;
  return typeof post.id === 'string'
    && (post.kind === 'Heading' || post.kind === 'Body')
    && typeof post.text === 'string'
    && typeof post.chapter === 'string';
}

export function postsFromJson(value: unknown): FeedPost[] | null {
  if (!Array.isArray(value) || !value.every(isPost)) return null;
  return value;
}

/** Use a saved feed when it exists. Otherwise download once and keep it. */
export async function loadCachedPosts(
  key: string,
  cache: PostCache,
  fetchPosts: () => Promise<FeedPost[]>,
): Promise<FeedPost[]> {
  const saved = await cache.read(key);
  if (saved) return saved;
  const posts = await fetchPosts();
  await cache.write(key, posts);
  return posts;
}

const DB_NAME = 'mewo';
const STORE = 'posts';

function openDb(): Promise<IDBDatabase> {
  return new Promise((resolve, reject) => {
    const request = indexedDB.open(DB_NAME, 1);
    request.onupgradeneeded = () => {
      const db = request.result;
      if (!db.objectStoreNames.contains(STORE)) db.createObjectStore(STORE);
    };
    request.onsuccess = () => resolve(request.result);
    request.onerror = () => reject(request.error);
  });
}

async function withStore<T>(
  mode: IDBTransactionMode,
  run: (store: IDBObjectStore) => Promise<T>,
): Promise<T> {
  const db = await openDb();
  try {
    return await run(db.transaction(STORE, mode).objectStore(STORE));
  } finally {
    db.close();
  }
}

function requestToPromise<T>(request: IDBRequest<T>): Promise<T> {
  return new Promise((resolve, reject) => {
    request.onsuccess = () => resolve(request.result);
    request.onerror = () => reject(request.error);
  });
}

/** IndexedDB feed cache. Missing IndexedDB (tests, locked-down browsers) is a miss. */
export const indexedPosts: PostCache = {
  async read(key: string): Promise<FeedPost[] | null> {
    if (typeof indexedDB === 'undefined') return null;
    try {
      const value = await withStore('readonly', (store) => requestToPromise(store.get(key)));
      return postsFromJson(value);
    } catch {
      return null;
    }
  },
  async write(key: string, posts: FeedPost[]): Promise<void> {
    if (typeof indexedDB === 'undefined') return;
    try {
      await withStore('readwrite', (store) => requestToPromise(store.put(posts, key)));
    } catch {
      // The feed stays in memory for this tab when the disk write fails.
    }
  },
  async delete(key: string): Promise<void> {
    if (typeof indexedDB === 'undefined') return;
    try {
      await withStore('readwrite', (store) => requestToPromise(store.delete(key)));
    } catch {
      // The in-memory feed is already gone.
    }
  },
  async clearPrefix(prefix: string): Promise<void> {
    if (typeof indexedDB === 'undefined') return;
    try {
      await withStore('readwrite', (store) => new Promise<void>((resolve, reject) => {
        const cursor = store.openCursor();
        cursor.onerror = () => reject(cursor.error);
        cursor.onsuccess = () => {
          const step = cursor.result;
          if (!step) { resolve(); return; }
          if (String(step.key).startsWith(prefix)) step.delete();
          step.continue();
        };
      }));
    } catch {
      // Sign-out still drops the in-memory feeds.
    }
  },
};

/** In-memory stand-in for tests and for callers that inject a cache. */
export class MemoryPosts implements PostCache {
  private readonly rows = new Map<string, FeedPost[]>();

  async read(key: string): Promise<FeedPost[] | null> {
    return this.rows.get(key) ?? null;
  }

  async write(key: string, posts: FeedPost[]): Promise<void> {
    this.rows.set(key, posts);
  }

  async delete(key: string): Promise<void> {
    this.rows.delete(key);
  }

  async clearPrefix(prefix: string): Promise<void> {
    for (const key of [...this.rows.keys()]) {
      if (key.startsWith(prefix)) this.rows.delete(key);
    }
  }
}
