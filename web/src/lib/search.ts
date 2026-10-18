import type { SearchKind, SearchPage } from './models';

export const SEARCH_DEBOUNCE_MS = 250;

/** Nothing to ask yet. An empty field is this, not "No matches". */
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

export function visiblePage(state: SearchState): SearchPage | undefined {
  if (state.phase.status === 'loaded') return state.phase.page;
  if (state.phase.status === 'loading') return state.phase.previous;
  if (state.phase.status === 'failed') return state.phase.previous;
  return undefined;
}

/** True only after a successful page for this query and tab came back empty. */
export function showNoMatches(state: SearchState): boolean {
  if (!state.query.trim()) return false;
  if (state.phase.status !== 'loaded' || state.phase.more) return false;
  return state.phase.page.type === state.kind && state.phase.page.items.length === 0;
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
  return next;
}

/**
 * One search box. Keystrokes and tab changes share a generation: the request
 * that belongs to an older one, or to a session that has since signed out,
 * cannot publish.
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

  constructor(
    fetchPage: SearchFetch,
    emit: (state: SearchState) => void = () => {},
    schedule: (fn: () => void, ms: number) => ReturnType<typeof setTimeout> = (fn, ms) => setTimeout(fn, ms),
    cancelTimer: (id: ReturnType<typeof setTimeout>) => void = (id) => clearTimeout(id),
    session: string | null = null,
    kind: SearchKind = 'accounts',
  ) {
    this.fetchPage = fetchPage;
    this.emit = emit;
    this.schedule = schedule;
    this.cancelTimer = cancelTimer;
    this.state = { query: '', kind, phase: { status: 'idle' }, session };
  }

  setSession(session: string | null): void {
    if (session === this.state.session) return;
    this.generation += 1;
    this.stop();
    this.pendingCursor = null;
    this.update({ ...this.state, session, phase: { status: 'idle' } });
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
    if (!trimmed || !this.state.session) {
      this.update({ ...this.state, query, phase: { status: 'idle' } });
      return;
    }
    const previous = visiblePage(this.state);
    const kept = previous?.type === this.state.kind ? previous : undefined;
    this.update({ ...this.state, query, phase: { status: 'loading', previous: kept } });
    this.arm(null);
  }

  setKind(kind: SearchKind): void {
    if (kind === this.state.kind) return;
    this.generation += 1;
    this.stop();
    this.pendingCursor = null;
    const previous = visiblePage(this.state);
    const kept = previous?.type === kind ? previous : undefined;
    const asking = !!this.state.query.trim() && !!this.state.session;
    this.update({
      ...this.state,
      kind,
      phase: asking ? { status: 'loading', previous: kept } : { status: 'idle' },
    });
    if (asking) this.arm(null);
  }

  retry(): void {
    if (this.state.phase.status !== 'failed') return;
    const session = this.state.session;
    const query = this.state.query.trim();
    if (!session || !query) return;
    const cursor = this.pendingCursor;
    const previous = this.state.phase.previous;
    this.generation += 1;
    this.stop();
    this.update({ ...this.state, phase: { status: 'loading', previous } });
    void this.run(this.generation, session, query, this.state.kind, cursor);
  }

  loadMore(): void {
    const phase = this.state.phase;
    if (phase.status !== 'loaded' || phase.more || !phase.page.nextCursor) return;
    const session = this.state.session;
    const query = this.state.query.trim();
    if (!session || !query) return;
    const cursor = phase.page.nextCursor;
    this.generation += 1;
    this.stop();
    this.pendingCursor = cursor;
    this.update({ ...this.state, phase: { status: 'loaded', page: phase.page, more: true } });
    void this.run(this.generation, session, query, this.state.kind, cursor);
  }

  private arm(cursor: string | null): void {
    const generation = this.generation;
    const session = this.state.session;
    const query = this.state.query.trim();
    const kind = this.state.kind;
    if (!session || !query) return;
    this.pendingCursor = cursor;
    this.timer = this.schedule(() => {
      this.timer = undefined;
      if (generation !== this.generation) return;
      void this.run(generation, session, query, kind, cursor);
    }, SEARCH_DEBOUNCE_MS);
  }

  private async run(
    generation: number,
    session: string,
    query: string,
    kind: SearchKind,
    cursor: string | null,
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
      this.update({ ...this.state, phase: { status: 'loaded', page: merged, more: false } });
    } catch (error) {
      if (abort.signal.aborted || !this.current(generation, session, query, kind)) return;
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
