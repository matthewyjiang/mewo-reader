import type { BookRequest, SearchKind, SearchPage } from './models';

export const SEARCH_DEBOUNCE_MS = 250;
export const CATALOG_DEBOUNCE_MS = 300;
export const BROWSE_REVALIDATE_MS = 60_000;
const MEMO_LIMIT = 8;

/** No session. A signed-in empty field is a browse request, not this. */
export interface SearchIdle {
  status: 'idle';
}

/** A request is out. `previous` stays on screen until the answer arrives. */
export interface SearchLoading {
  status: 'loading';
  previous?: SearchPage;
}

/** `more` is a next-page request. The page already on screen stays put. */
export interface SearchLoaded {
  status: 'loaded';
  page: SearchPage;
  more: boolean;
}

/** `previous` is the last page still worth showing. Retry uses the same query. */
export interface SearchFailed {
  status: 'failed';
  error: string;
  previous?: SearchPage;
}

export type SearchPhase = SearchIdle | SearchLoading | SearchLoaded | SearchFailed;

export interface SearchState {
  query: string;
  kind: SearchKind;
  phase: SearchPhase;
  /** Bearer token, or null after sign-out. A response for another token is dropped. */
  session: string | null;
}

export interface SearchRequest {
  query: string;
  kind: SearchKind;
  cursor: string | null;
  session: string;
  signal: AbortSignal;
}

export type SearchFetch = (request: SearchRequest) => Promise<SearchPage>;

interface MemoEntry {
  page: SearchPage;
  at: number;
  /** Responses concatenated into `page`. A quiet refresh is one response. */
  pages: number;
}

const memos = new Map<string, Map<string, MemoEntry>>();

/** Pages for one signed-in session. A token change drops the previous map. */
export function searchMemo(token: string): Map<string, MemoEntry> {
  let memo = memos.get(token);
  if (!memo) {
    memo = new Map();
    memos.set(token, memo);
  }
  return memo;
}

function dropMemo(token: string): void {
  const memo = memos.get(token);
  memo?.clear();
  memos.delete(token);
}

/** Sign-out drops every session's pages, including when Search is not mounted. */
export function clearSearchMemos(): void {
  memos.clear();
}

function memoKey(kind: SearchKind, query: string): string {
  return `${kind}\u0000${query}`;
}

export function visiblePage(state: SearchState): SearchPage | undefined {
  if (state.phase.status === 'loaded') return state.phase.page;
  if (state.phase.status === 'loading') return state.phase.previous;
  if (state.phase.status === 'failed') return state.phase.previous;
  return undefined;
}

/** An empty field is browse, including a field of only spaces. */
export function isBrowse(state: SearchState): boolean {
  return !state.query.trim();
}

export type EmptyKind = 'none' | 'noMatches' | 'nothingYet';

/** A finished empty page. Browse and a typed query use different copy. */
export function emptyKind(state: SearchState): EmptyKind {
  const phase = state.phase;
  if (phase.status !== 'loaded' || phase.more) return 'none';
  if (phase.page.type !== state.kind || phase.page.items.length > 0) return 'none';
  return isBrowse(state) ? 'nothingYet' : 'noMatches';
}

export function sectionFor(kind: SearchKind, languages: string[]): { title: string; note?: string } {
  if (kind === 'books') return { title: 'Popular on this server' };
  if (kind === 'accounts') return { title: 'Suggested accounts' };
  if (kind === 'posts') return {
    title: 'Top posts',
    note: 'Most replied and reposted in the last 30 days',
  };
  return { title: 'Trending on Open Library', note: trendingNote(languages) };
}

export function trendingNote(names: string[]): string {
  if (names.length === 0) return 'This week';
  const list = names.length === 1
    ? names[0]
    : `${names.slice(0, -1).join(', ')} and ${names[names.length - 1]}`;
  return `This week · showing books with editions in ${list}`;
}

export function concatPages(current: SearchPage, next: SearchPage): SearchPage {
  if (current.type !== next.type) return next;
  if (current.type === 'accounts' && next.type === 'accounts') {
    return { type: 'accounts', items: [...current.items, ...next.items], nextCursor: next.nextCursor };
  }
  if (current.type === 'books' && next.type === 'books') {
    return { type: 'books', items: [...current.items, ...next.items], nextCursor: next.nextCursor };
  }
  if (current.type === 'posts' && next.type === 'posts') {
    return { type: 'posts', items: [...current.items, ...next.items], nextCursor: next.nextCursor };
  }
  if (current.type === 'catalog' && next.type === 'catalog') {
    return { type: 'catalog', items: [...current.items, ...next.items], nextCursor: next.nextCursor };
  }
  return next;
}

/**
 * One search box. Keystrokes and tab changes share a generation: the request
 * that belongs to an older one, or to a session that has since signed out,
 * cannot publish. A blank query asks for that tab's browse page.
 */
export class SearchModel {
  state: SearchState;
  private generation = 0;
  private timer: ReturnType<typeof setTimeout> | undefined;
  private abort: AbortController | undefined;
  /** Cursor the failed request was asking for. Retry continues that page. */
  private pendingCursor: string | null = null;
  private readonly fetchPage: SearchFetch;
  private readonly emit: (state: SearchState) => void;
  private readonly schedule: (fn: () => void, ms: number) => ReturnType<typeof setTimeout>;
  private readonly cancelTimer: (id: ReturnType<typeof setTimeout>) => void;
  private readonly now: () => number;

  constructor(
    fetchPage: SearchFetch,
    emit: (state: SearchState) => void = () => {},
    schedule: (fn: () => void, ms: number) => ReturnType<typeof setTimeout> = (fn, ms) => setTimeout(fn, ms),
    cancelTimer: (id: ReturnType<typeof setTimeout>) => void = (id) => clearTimeout(id),
    session: string | null = null,
    kind: SearchKind = 'accounts',
    now: () => number = () => Date.now(),
    initialQuery = '',
  ) {
    this.fetchPage = fetchPage;
    this.emit = emit;
    this.schedule = schedule;
    this.cancelTimer = cancelTimer;
    this.now = now;
    this.state = { query: initialQuery, kind, phase: { status: 'idle' }, session };
    if (session) this.publishCurrent();
  }

  setSession(session: string | null): void {
    if (session === this.state.session) return;
    const previous = this.state.session;
    if (previous) dropMemo(previous);
    this.generation += 1;
    this.stop();
    this.pendingCursor = null;
    this.state = { ...this.state, session };
    if (!session) {
      this.update({ ...this.state, phase: { status: 'idle' } });
      return;
    }
    this.publishCurrent();
  }

  setQuery(query: string): void {
    const trimmed = query.trim();
    if (trimmed === this.state.query.trim()) {
      if (query !== this.state.query) this.update({ ...this.state, query });
      return;
    }
    this.generation += 1;
    this.stop();
    this.pendingCursor = null;
    if (!this.state.session) {
      this.update({ ...this.state, query, phase: { status: 'idle' } });
      return;
    }
    const hit = this.recall(this.state.kind, trimmed);
    if (hit) {
      this.update({ ...this.state, query, phase: { status: 'loaded', page: hit.page, more: false } });
      if (this.stale(hit, trimmed)) this.arm(null, true);
      return;
    }
    const previous = visiblePage(this.state);
    // Browse has its own heading. Another query's hits must not sit under it.
    const kept = trimmed && previous?.type === this.state.kind ? previous : undefined;
    this.update({ ...this.state, query, phase: { status: 'loading', previous: kept } });
    this.arm(null);
  }

  setKind(kind: SearchKind): void {
    if (kind === this.state.kind) return;
    this.generation += 1;
    this.stop();
    this.pendingCursor = null;
    if (!this.state.session) {
      this.update({ ...this.state, kind, phase: { status: 'idle' } });
      return;
    }
    const trimmed = this.state.query.trim();
    const hit = this.recall(kind, trimmed);
    if (hit) {
      this.update({ ...this.state, kind, phase: { status: 'loaded', page: hit.page, more: false } });
      if (this.stale(hit, trimmed)) this.arm(null, true);
      return;
    }
    const previous = visiblePage(this.state);
    const kept = previous?.type === kind ? previous : undefined;
    this.update({ ...this.state, kind, phase: { status: 'loading', previous: kept } });
    this.arm(null);
  }

  retry(): void {
    if (this.state.phase.status !== 'failed') return;
    const session = this.state.session;
    if (!session) return;
    const query = this.state.query.trim();
    const cursor = this.pendingCursor;
    const previous = this.state.phase.previous;
    this.generation += 1;
    this.stop();
    this.update({ ...this.state, phase: { status: 'loading', previous } });
    void this.run(this.generation, session, query, this.state.kind, cursor, false);
  }

  loadMore(): void {
    const phase = this.state.phase;
    if (phase.status !== 'loaded' || phase.more || !phase.page.nextCursor) return;
    const session = this.state.session;
    if (!session) return;
    const query = this.state.query.trim();
    const cursor = phase.page.nextCursor;
    this.generation += 1;
    this.stop();
    this.pendingCursor = cursor;
    this.update({ ...this.state, phase: { status: 'loaded', page: phase.page, more: true } });
    void this.run(this.generation, session, query, this.state.kind, cursor, false);
  }

  /** Keep a request the sheet learned about on the cached catalog page. */
  noteRequest(workKey: string, request: BookRequest | null): void {
    const session = this.state.session;
    if (!session) return;
    const apply = (page: SearchPage): SearchPage => {
      if (page.type !== 'catalog') return page;
      if (!page.items.some((work) => work.workKey === workKey)) return page;
      return {
        type: 'catalog',
        nextCursor: page.nextCursor,
        items: page.items.map((work) => work.workKey === workKey ? { ...work, request } : work),
      };
    };
    const memo = searchMemo(session);
    for (const [key, entry] of memo) {
      const page = apply(entry.page);
      if (page !== entry.page) memo.set(key, { ...entry, page });
    }
    const phase = this.state.phase;
    if (phase.status === 'loaded') {
      const page = apply(phase.page);
      if (page !== phase.page) this.update({ ...this.state, phase: { ...phase, page } });
      return;
    }
    if ((phase.status === 'loading' || phase.status === 'failed') && phase.previous) {
      const previous = apply(phase.previous);
      if (previous !== phase.previous) {
        this.update({ ...this.state, phase: { ...phase, previous } });
      }
    }
  }

  private publishCurrent(): void {
    const session = this.state.session;
    if (!session) {
      this.update({ ...this.state, phase: { status: 'idle' } });
      return;
    }
    const trimmed = this.state.query.trim();
    const hit = this.recall(this.state.kind, trimmed);
    if (hit) {
      this.update({ ...this.state, phase: { status: 'loaded', page: hit.page, more: false } });
      if (this.stale(hit, trimmed)) this.arm(null, true);
      return;
    }
    this.update({ ...this.state, phase: { status: 'loading' } });
    this.arm(null);
  }

  private recall(kind: SearchKind, query: string): MemoEntry | undefined {
    const session = this.state.session;
    if (!session) return undefined;
    return searchMemo(session).get(memoKey(kind, query));
  }

  /**
   * A quiet refresh asks for the first page only, so a memo built from more
   * than one response stays as it is. Catalog request status changes while
   * the query does not, so a single catalog page refreshes whenever it is shown.
   */
  private stale(entry: MemoEntry, query: string): boolean {
    if (entry.pages > 1) return false;
    if (entry.page.type === 'catalog') return true;
    if (query) return false;
    return this.now() - entry.at > BROWSE_REVALIDATE_MS;
  }

  private remember(page: SearchPage, pages: number): void {
    const session = this.state.session;
    if (!session) return;
    const key = memoKey(page.type, this.state.query.trim());
    const memo = searchMemo(session);
    if (memo.has(key)) memo.delete(key);
    memo.set(key, { page, at: this.now(), pages });
    while (memo.size > MEMO_LIMIT) {
      const oldest = memo.keys().next().value;
      if (oldest === undefined) break;
      memo.delete(oldest);
    }
  }

  private arm(cursor: string | null, quiet = false): void {
    const generation = this.generation;
    const session = this.state.session;
    const query = this.state.query.trim();
    const kind = this.state.kind;
    if (!session) return;
    this.pendingCursor = cursor;
    const delay = quiet || !query
      ? 0
      : (kind === 'catalog' ? CATALOG_DEBOUNCE_MS : SEARCH_DEBOUNCE_MS);
    this.timer = this.schedule(() => {
      this.timer = undefined;
      if (generation !== this.generation) return;
      void this.run(generation, session, query, kind, cursor, quiet);
    }, delay);
  }

  private async run(
    generation: number,
    session: string,
    query: string,
    kind: SearchKind,
    cursor: string | null,
    quiet: boolean,
  ): Promise<void> {
    if (!this.current(generation, session, query, kind)) return;
    const abort = new AbortController();
    this.abort = abort;
    this.pendingCursor = cursor;
    try {
      const page = await this.fetchPage({ query, kind, cursor, session, signal: abort.signal });
      if (abort.signal.aborted || !this.current(generation, session, query, kind)) return;
      if (page.type !== kind) return;
      const shown = visiblePage(this.state);
      const merged = cursor && shown && shown.type === page.type ? concatPages(shown, page) : page;
      const prior = cursor ? (this.recall(kind, query)?.pages ?? 1) : 0;
      this.remember(merged, cursor ? prior + 1 : 1);
      this.update({ ...this.state, phase: { status: 'loaded', page: merged, more: false } });
    } catch (error) {
      if (abort.signal.aborted || !this.current(generation, session, query, kind)) return;
      if (quiet && visiblePage(this.state)) return;
      const previous = visiblePage(this.state);
      const message = error instanceof Error && error.message
        ? error.message
        : 'Something went wrong. Try again.';
      this.update({ ...this.state, phase: { status: 'failed', error: message, previous } });
    } finally {
      if (this.abort === abort) this.abort = undefined;
    }
  }

  private current(generation: number, session: string, query: string, kind: SearchKind): boolean {
    return generation === this.generation
      && session === this.state.session
      && query === this.state.query.trim()
      && kind === this.state.kind;
  }

  private stop(): void {
    if (this.timer !== undefined) {
      this.cancelTimer(this.timer);
      this.timer = undefined;
    }
    this.abort?.abort();
    this.abort = undefined;
  }

  private update(state: SearchState): void {
    this.state = state;
    this.emit(state);
  }
}
