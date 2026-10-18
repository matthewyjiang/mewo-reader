import type { HomeFeedItem, HomeFeedPage } from './models';

/** Not loaded yet. Empty copy is not this state. */
export interface FeedLoading {
  status: 'loading';
}

/** A fetch has succeeded. `refreshing` reloads [page]. `more` appends the next cursor. */
export interface FeedLoaded {
  status: 'loaded';
  page: HomeFeedPage;
  refreshing: boolean;
  more: boolean;
}

/** The last fetch failed. `page` is the previous success, when there was one. */
export interface FeedFailed {
  status: 'failed';
  error: string;
  page: HomeFeedPage | null;
}

export type FeedPhase = FeedLoading | FeedLoaded | FeedFailed;

export function showFeedEmpty(phase: FeedPhase): boolean {
  if (phase.status === 'loading') return false;
  const page = phase.page;
  return page !== null && page.items.length === 0;
}

export function beginFeedRefresh(phase: FeedPhase): FeedPhase {
  if (phase.status === 'loaded') return { ...phase, refreshing: true, more: false };
  if (phase.status === 'failed' && phase.page) {
    return { status: 'loaded', page: phase.page, refreshing: true, more: false };
  }
  return { status: 'loading' };
}

export function finishFeedRefresh(page: HomeFeedPage): FeedPhase {
  return { status: 'loaded', page, refreshing: false, more: false };
}

export function failFeed(phase: FeedPhase, error: string): FeedPhase {
  const page = phase.status === 'loading' ? null : phase.page;
  return { status: 'failed', error, page };
}

export function startFeedMore(phase: FeedPhase): FeedPhase {
  if (phase.status !== 'loaded' || !phase.page.nextCursor) return phase;
  return { ...phase, more: true };
}

export function appendFeed(phase: FeedPhase, page: HomeFeedPage): FeedPhase {
  const current = phase.status === 'loaded' ? phase.page : phase.status === 'failed' ? phase.page : null;
  const items: HomeFeedItem[] = current ? [...current.items, ...page.items] : page.items;
  return {
    status: 'loaded',
    page: { items, nextCursor: page.nextCursor },
    refreshing: false,
    more: false,
  };
}

export function phaseFromFeedCache(page: HomeFeedPage | null): FeedPhase {
  if (!page) return { status: 'loading' };
  return { status: 'loaded', page, refreshing: true, more: false };
}

/** Page 2 of a cached cursor must not run while the first page is refreshing. */
export function feedMoreAllowed(
  phase: FeedPhase,
): phase is FeedLoaded & { page: HomeFeedPage & { nextCursor: string } } {
  return phase.status === 'loaded' && !phase.refreshing && !phase.more && !!phase.page.nextCursor;
}

/** One feed per origin (or server URL) and username. */
export function feedKey(server: string, username: string): string {
  return `mewo.feed.${encodeURIComponent(server)}.${encodeURIComponent(username)}`;
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

function isItem(value: unknown): value is HomeFeedItem {
  if (!value || typeof value !== 'object' || !('type' in value)) return false;
  const item = value as { type?: unknown };
  return item.type === 'comment' || item.type === 'repost';
}

export function readFeed(
  server: string,
  username: string,
  store: KeyValueStore | null = storage(),
): HomeFeedPage | null {
  if (!server || !username || !store) return null;
  try {
    const raw = store.getItem(feedKey(server, username));
    if (!raw) return null;
    const parsed: unknown = JSON.parse(raw);
    if (!parsed || typeof parsed !== 'object') return null;
    const page = parsed as { items?: unknown; nextCursor?: unknown };
    if (!Array.isArray(page.items) || !page.items.every(isItem)) return null;
    const nextCursor = typeof page.nextCursor === 'string' ? page.nextCursor : null;
    return { items: page.items, nextCursor };
  } catch {
    return null;
  }
}

export function writeFeed(
  server: string,
  username: string,
  page: HomeFeedPage,
  store: KeyValueStore | null = storage(),
): void {
  if (!server || !username || !store) return;
  try {
    store.setItem(feedKey(server, username), JSON.stringify(page));
  } catch {
    // Private browsing can refuse the write. The in-memory feed still works.
  }
}

export function clearFeed(
  server: string,
  username: string,
  store: KeyValueStore | null = storage(),
): void {
  if (!server || !username || !store) return;
  try {
    store.removeItem(feedKey(server, username));
  } catch {
    // Ignore a blocked storage API.
  }
}
