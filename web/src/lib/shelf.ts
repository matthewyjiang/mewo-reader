import type { BookRecord } from './models';

/** Not loaded yet. Empty copy is not this state. */
export interface ShelfLoading {
  status: 'loading';
}

/** A fetch has succeeded. `refreshing` is a background reload of [books]. */
export interface ShelfLoaded {
  status: 'loaded';
  books: BookRecord[];
  refreshing: boolean;
}

/** The last fetch failed. `books` is the previous success, when there was one. */
export interface ShelfFailed {
  status: 'failed';
  error: string;
  books: BookRecord[] | null;
}

export type LibraryPhase = ShelfLoading | ShelfLoaded | ShelfFailed;

export function shelfBooks(phase: LibraryPhase): BookRecord[] {
  if (phase.status === 'loading') return [];
  return phase.books ?? [];
}

/** True only after a successful fetch returned nothing, including a later failed refresh. */
export function showEmpty(phase: LibraryPhase): boolean {
  if (phase.status === 'loading') return false;
  return phase.books !== null && phase.books.length === 0;
}

/** Ids a successful refresh dropped. Their saved text should go with them. */
export function droppedBookIds(before: string[], after: string[]): string[] {
  const kept = new Set(after);
  return before.filter((id) => !kept.has(id));
}

/**
 * Saved text may open a book only while the shelf is still catching up, or
 * when a finished shelf still lists it. A completed refresh is the list.
 */
export function savedTextAllowed(phase: LibraryPhase, bookId: string): boolean {
  if (phase.status === 'loading') return true;
  if (phase.status === 'loaded' && phase.refreshing) return true;
  if (phase.status === 'failed' && phase.books == null) return true;
  return shelfBooks(phase).some((book) => book.id === bookId);
}

export function sortShelf(books: BookRecord[]): BookRecord[] {
  return [...books].sort((a, b) => b.importedAt - a.importedAt);
}

export function beginRefresh(phase: LibraryPhase): LibraryPhase {
  if (phase.status === 'loaded') return { ...phase, refreshing: true };
  if (phase.status === 'failed' && phase.books) {
    return { status: 'loaded', books: phase.books, refreshing: true };
  }
  return { status: 'loading' };
}

export function finishRefresh(books: BookRecord[]): LibraryPhase {
  return { status: 'loaded', books, refreshing: false };
}

export function failRefresh(phase: LibraryPhase, error: string): LibraryPhase {
  const books = phase.status === 'loading' ? null : phase.books;
  return { status: 'failed', error, books };
}

export function replaceBooks(phase: LibraryPhase, books: BookRecord[]): LibraryPhase {
  if (phase.status === 'loaded') return { ...phase, books };
  if (phase.status === 'failed') return { ...phase, books };
  return { status: 'loaded', books, refreshing: false };
}

export function dismissShelfError(phase: LibraryPhase): LibraryPhase {
  if (phase.status === 'failed' && phase.books) {
    return { status: 'loaded', books: phase.books, refreshing: false };
  }
  return phase;
}

/** One shelf per origin (or server URL) and username. */
export function shelfKey(server: string, username: string): string {
  return `mewo.shelf.${encodeURIComponent(server)}.${encodeURIComponent(username)}`;
}

export interface KeyValueStore {
  getItem(key: string): string | null;
  setItem(key: string, value: string): void;
  removeItem(key: string): void;
}

function storage(): KeyValueStore | null {
  try {
    return globalThis.localStorage ?? null;
  } catch {
    return null;
  }
}

function isBook(value: unknown): value is BookRecord {
  if (!value || typeof value !== 'object') return false;
  const book = value as Partial<BookRecord>;
  return typeof book.id === 'string'
    && typeof book.title === 'string'
    && typeof book.author === 'string'
    && typeof book.handle === 'string'
    && typeof book.importedAt === 'number'
    && typeof book.postCount === 'number'
    && typeof book.progressIndex === 'number'
    && typeof book.isSample === 'boolean'
    && typeof book.mine === 'boolean'
    && typeof book.shared === 'boolean'
    && (book.ownerUsername === undefined || typeof book.ownerUsername === 'string');
}

export function readShelf(
  server: string,
  username: string,
  store: KeyValueStore | null = storage(),
): BookRecord[] | null {
  if (!server || !username || !store) return null;
  try {
    const raw = store.getItem(shelfKey(server, username));
    if (!raw) return null;
    const parsed: unknown = JSON.parse(raw);
    if (!Array.isArray(parsed) || !parsed.every(isBook)) return null;
    return sortShelf(parsed);
  } catch {
    return null;
  }
}

export function writeShelf(
  server: string,
  username: string,
  books: BookRecord[],
  store: KeyValueStore | null = storage(),
): void {
  if (!server || !username || !store) return;
  try {
    store.setItem(shelfKey(server, username), JSON.stringify(books));
  } catch {
    // Private browsing can refuse the write. The in-memory shelf still works.
  }
}

export function clearShelf(
  server: string,
  username: string,
  store: KeyValueStore | null = storage(),
): void {
  if (!server || !username || !store) return;
  try {
    store.removeItem(shelfKey(server, username));
  } catch {
    // Ignore a blocked storage API.
  }
}

export function phaseFromCache(books: BookRecord[] | null): LibraryPhase {
  if (!books) return { status: 'loading' };
  return { status: 'loaded', books, refreshing: true };
}

/** Sign-out or a newer session beat this refresh. The caller already reset the phase. */
export class ShelfCancelled extends Error {
  constructor() {
    super('Shelf refresh was cancelled.');
    this.name = 'ShelfCancelled';
  }
}

/**
 * One library refresh. A second call joins the first so a pull during launch
 * reuses that request. The next call, after it finishes, hits the server again.
 */
export class ShelfRefresh {
  phase: LibraryPhase;
  private flight: Promise<void> | null = null;
  private readonly onChange: (phase: LibraryPhase) => void;

  constructor(phase: LibraryPhase, onChange: (phase: LibraryPhase) => void) {
    this.phase = phase;
    this.onChange = onChange;
  }

  async refresh(fetchBooks: () => Promise<BookRecord[]>): Promise<void> {
    if (this.flight) return this.flight;
    const run = (async () => {
      this.set(beginRefresh(this.phase));
      try {
        this.set(finishRefresh(sortShelf(await fetchBooks())));
      } catch (error) {
        if (error instanceof ShelfCancelled) return;
        const message = error instanceof Error && error.message
          ? error.message
          : 'Could not load the library.';
        this.set(failRefresh(this.phase, message));
      }
    })();
    const flight = run.finally(() => {
      if (this.flight === flight) this.flight = null;
    });
    this.flight = flight;
    return flight;
  }

  replace(books: BookRecord[]): void {
    this.set(replaceBooks(this.phase, sortShelf(books)));
  }

  /** Show a saved shelf immediately, then let [refresh] replace it. */
  prime(books: BookRecord[]): void {
    if (this.phase.status !== 'loading') return;
    this.set({ status: 'loaded', books: sortShelf(books), refreshing: true });
  }

  dismissError(): void {
    this.set(dismissShelfError(this.phase));
  }

  reset(): void {
    this.flight = null;
    this.set({ status: 'loading' });
  }

  private set(phase: LibraryPhase): void {
    this.phase = phase;
    this.onChange(phase);
  }
}
