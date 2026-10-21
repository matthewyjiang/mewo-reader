<script lang="ts">
  import { onMount, tick, untrack } from 'svelte';
  import BookRow from '../components/BookRow.svelte';
  import CatalogRow from '../components/CatalogRow.svelte';
  import CatalogSheet from '../components/CatalogSheet.svelte';
  import Post from '../components/Post.svelte';
  import SearchEmpty from '../components/SearchEmpty.svelte';
  import SearchSection from '../components/SearchSection.svelte';
  import SearchSkeleton from '../components/SearchSkeleton.svelte';
  import UnderlineTabs from '../components/UnderlineTabs.svelte';
  import Avatar from '../components/Avatar.svelte';
  import { ApiError } from '../lib/api';
  import type { BookRequest, CatalogWork, SearchKind } from '../lib/models';
  import { message } from '../lib/store.svelte';
  import { catalogErrorText, rowAction } from '../lib/requests';
  import { readerUrl, router } from '../lib/router.svelte';
  import { restoreScroll } from '../lib/screen';
  import type { FeedScreenProps } from '../lib/screen';
  import { emptyKind, isBrowse, SearchModel, sectionFor, visiblePage } from '../lib/search';
  import type { SearchState } from '../lib/search';
  import { readStored, writeStored } from '../lib/storage';
  import { store } from '../lib/store.svelte';

  let { scroller, query, oncomment, onprofile, onadd }: FeedScreenProps & {
    query: string;
    onprofile: (username: string) => void;
    onadd: () => void;
  } = $props();

  const saved = readStored<string>('mewo.search.kind', 'accounts');
  const initialKind: SearchKind = saved === 'books' || saved === 'posts' || saved === 'catalog'
    ? saved
    : 'accounts';
  const entryKey = untrack(() => router.current.key);
  const initialQuery = untrack(() => query);
  let searchState = $state<SearchState>({
    query: initialQuery,
    kind: initialKind,
    phase: { status: 'idle' },
    session: store.session?.token ?? null,
  });
  let overrides = $state<Record<string, BookRequest | null>>({});
  let openWork = $state<CatalogWork | null>(null);
  let restoredCatalog = false;
  const scrollByKind = new Map<SearchKind, number>();
  let restoreKind = $state<SearchKind | null>(null);
  let restoreY = 0;
  const model = new SearchModel(
    async (request) => {
      try {
        if (request.kind !== 'catalog') {
          return store.api.search(request.query, request.kind, request.cursor, request.signal);
        }
        const pageNumber = request.cursor ? Number(request.cursor) : 1;
        const result = await store.api.catalog(
          request.query,
          Number.isFinite(pageNumber) && pageNumber > 0 ? pageNumber : 1,
          request.signal,
        );
        return {
          type: 'catalog' as const,
          items: result.items,
          nextCursor: result.nextPage == null ? null : String(result.nextPage),
        };
      } catch (error) {
        if (request.kind !== 'catalog') throw error;
        if (error instanceof DOMException && error.name === 'AbortError') throw error;
        const status = error instanceof ApiError ? error.status : -1;
        const text = error instanceof Error && error.message
          ? error.message
          : 'Something went wrong. Try again.';
        throw new Error(catalogErrorText(status, text));
      }
    },
    (next) => { searchState = next; },
    (fn, ms) => setTimeout(fn, ms),
    (id) => clearTimeout(id),
    store.session?.token ?? null,
    initialKind,
    () => Date.now(),
    initialQuery,
  );

  let canRequest = $derived(store.me?.canRequest === true);
  let isAdmin = $derived(store.me?.isAdmin === true);
  let tabs = $derived([
    { id: 'accounts', label: 'Accounts' },
    { id: 'books', label: 'Books' },
    { id: 'posts', label: 'Posts' },
    ...(canRequest ? [{ id: 'catalog', label: 'Catalog' }] : []),
  ]);
  let page = $derived(visiblePage(searchState));
  let browse = $derived(isBrowse(searchState));
  let section = $derived.by(() => {
    if (!browse || !searchState.session) return null;
    if (searchState.kind === 'catalog') {
      const rows = page?.items.length ?? 0;
      const loadingBare = searchState.phase.status === 'loading' && !page;
      if (!loadingBare && rows === 0) return null;
    }
    return sectionFor(searchState.kind);
  });
  let blank = $derived(emptyKind(searchState));
  let showSkeleton = $derived(searchState.phase.status === 'loading' && !page);

  export function capture(): void {
    router.scroll.set(entryKey, scroller.scrollTop);
    router.tabScroll.set('search', scroller.scrollTop);
  }

  onMount(() => restoreScroll(scroller));

  $effect(() => {
    const token = store.session?.token ?? null;
    untrack(() => model.setSession(token));
  });

  $effect(() => {
    const text = query;
    untrack(() => {
      if (text.trim() !== searchState.query.trim()) {
        overrides = {};
        openWork = null;
      }
      model.setQuery(text);
    });
  });

  $effect(() => {
    const me = store.me;
    const can = canRequest;
    untrack(() => {
      if (me == null || restoredCatalog) return;
      restoredCatalog = true;
      if (saved === 'catalog' && can && searchState.kind !== 'catalog') model.setKind('catalog');
      if (!can && searchState.kind === 'catalog') model.setKind('accounts');
    });
  });

  $effect(() => {
    const kind = searchState.kind;
    const pending = restoreKind;
    const status = searchState.phase.status;
    const count = page?.type === kind ? page.items.length : -1;
    if (pending !== kind) return;
    void tick().then(() => {
      if (restoreKind !== kind || searchState.kind !== kind) return;
      scroller.scrollTop = restoreY;
      if ((status === 'loaded' || status === 'failed') && count >= 0) restoreKind = null;
    });
  });

  async function select(id: string): Promise<void> {
    if (id !== 'accounts' && id !== 'books' && id !== 'posts' && id !== 'catalog') return;
    if (id === 'catalog' && !canRequest) return;
    if (id === searchState.kind) return;
    scrollByKind.set(searchState.kind, scroller.scrollTop);
    writeStored('mewo.search.kind', id);
    openWork = null;
    model.setKind(id);
    restoreKind = id;
    restoreY = scrollByKind.get(id) ?? 0;
    await tick();
    if (searchState.kind === id) scroller.scrollTop = restoreY;
  }

  function shown(work: CatalogWork): BookRequest | null {
    if (Object.hasOwn(overrides, work.workKey)) return overrides[work.workKey];
    return work.request;
  }

  function onRequest(workKey: string, next: BookRequest | null): void {
    overrides[workKey] = next;
    model.noteRequest(workKey, next);
  }

  async function act(work: CatalogWork): Promise<void> {
    const current = shown(work);
    const action = rowAction(current, work.onServer);
    try {
      if (action === 'request' || (action === 'again' && current?.state !== 'canceled')) {
        onRequest(work.workKey, await store.api.requestBook(work));
      } else if ((action === 'retry' || action === 'again') && current) {
        onRequest(work.workKey, await store.api.retryRequest(current.id));
      } else if (action === 'open') {
        const id = current?.bookId || work.onServer?.bookId;
        if (id) router.go(readerUrl(id));
      }
    } catch (error) {
      store.notice = message(error);
    }
  }

  let socialIds = $derived(page?.type === 'posts' ? [...new Set(page.items.map((hit) => hit.book.id))] : []);
  let socialKey = $derived(JSON.stringify(socialIds));
  $effect(() => {
    socialKey;
    untrack(() => {
      for (const id of socialIds) void store.social(id).catch(() => {});
    });
  });

  function sentinel(node: HTMLElement): { destroy: () => void } {
    const observer = new IntersectionObserver((entries) => {
      if (entries.some((entry) => entry.isIntersecting)) model.loadMore();
    }, { root: scroller });
    observer.observe(node);
    return { destroy: () => observer.disconnect() };
  }
</script>

<UnderlineTabs {tabs} selected={searchState.kind} fixed onselect={(id) => { void select(id); }} />

{#if searchState.phase.status === 'failed'}
  <div class="status-row" class:catalog={searchState.kind === 'catalog'}>
    <span class="error" role="alert">{searchState.phase.error}</span>
    <button
      class="text-button"
      class:retry={searchState.kind === 'catalog'}
      type="button"
      onclick={() => model.retry()}
    >Try again</button>
  </div>
{/if}
{#if section}
  <SearchSection title={section.title} />
{/if}
{#if showSkeleton}
  <SearchSkeleton kind={searchState.kind} />
{/if}
{#if page?.type === 'accounts'}
  {#each page.items as account (account.username)}
    <button class="account-row" type="button" onclick={() => onprofile(account.username)}>
      <Avatar name={account.username} size={40} />
      <span class="account-names">
        <strong>{account.username}</strong>
        <span class="meta">@{account.username}</span>
      </span>
    </button>
  {/each}
{:else if page?.type === 'books'}
  {#each page.items as book (book.id)}
    <BookRow {book} />
  {/each}
{:else if page?.type === 'catalog'}
  {#each page.items as work (work.workKey)}
    <CatalogRow
      {work}
      request={shown(work)}
      onopen={() => openWork = work}
      onact={() => { void act(work); }}
    />
  {/each}
{:else if page?.type === 'posts'}
  {#each page.items as hit (`${hit.book.id}:${hit.post.id}`)}
    <Post
      book={hit.book}
      post={hit.post}
      onopen={() => router.go(readerUrl(hit.book.id, hit.post.id))}
      oncomment={() => oncomment(hit.book, hit.post)}
    />
  {/each}
{/if}
{#if searchState.phase.status === 'loaded' && searchState.phase.more}
  <div class="status-row" role="status">
    <span class="spinner"></span>
    <span class="sr-only">Loading more…</span>
  </div>
{/if}
{#if searchState.phase.status === 'loaded' && searchState.phase.page.nextCursor && !searchState.phase.more}
  <div use:sentinel></div>
{/if}
{#if blank !== 'none'}
  <SearchEmpty
    kind={searchState.kind}
    mode={blank}
    query={searchState.query}
    {canRequest}
    {isAdmin}
    onkind={(id) => { void select(id); }}
    {onadd}
  />
{/if}
{#if openWork}
  {@const work = openWork}
  <CatalogSheet
    {work}
    request={shown(work)}
    onchange={(next) => onRequest(work.workKey, next)}
    onclose={() => openWork = null}
  />
{/if}

<style>
  .status-row.catalog .error {
    flex: 1;
    line-height: 20px;
  }

  .retry {
    font-size: .9375rem;
    font-weight: 700;
  }
</style>
