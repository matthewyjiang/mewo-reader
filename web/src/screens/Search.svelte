<script lang="ts">
  import { onMount, untrack } from 'svelte';
  import BookRow from '../components/BookRow.svelte';
  import Icon from '../components/Icon.svelte';
  import Post from '../components/Post.svelte';
  import UnderlineTabs from '../components/UnderlineTabs.svelte';
  import Avatar from '../components/Avatar.svelte';
  import BookCover from '../components/BookCover.svelte';
  import type { BookRequest, CatalogWork, SearchKind } from '../lib/models';
  import { message } from '../lib/store.svelte';
  import { coverUrl, rowAction, stateLabel } from '../lib/requests';
  import { readerUrl, router } from '../lib/router.svelte';
  import { restoreScroll } from '../lib/screen';
  import type { FeedScreenProps } from '../lib/screen';
  import { SearchModel, showNoMatches, visiblePage } from '../lib/search';
  import type { SearchState } from '../lib/search';
  import { readStored, writeStored } from '../lib/storage';
  import { store } from '../lib/store.svelte';

  let { scroller, query, oncomment, onprofile }: FeedScreenProps & {
    query: string;
    onprofile: (username: string) => void;
  } = $props();

  const saved = readStored<string>('mewo.search.kind', 'accounts');
  const initialKind: SearchKind = saved === 'books' || saved === 'posts' || saved === 'catalog'
    ? saved
    : 'accounts';
  const entryKey = untrack(() => router.current.key);
  let searchState = $state<SearchState>({
    query: '',
    kind: initialKind,
    phase: { status: 'idle' },
    session: store.session?.token ?? null,
  });
  let overrides = $state<Record<string, BookRequest>>({});
  let restoredCatalog = false;
  const model = new SearchModel(
    async (request) => {
      if (request.kind !== 'catalog') {
        return store.api.search(request.query, request.kind, request.cursor, request.signal);
      }
      const page = request.cursor ? Number(request.cursor) : 1;
      const result = await store.api.catalog(
        request.query,
        Number.isFinite(page) && page > 0 ? page : 1,
        request.signal,
      );
      return {
        type: 'catalog' as const,
        items: result.items,
        nextCursor: result.nextPage == null ? null : String(result.nextPage),
      };
    },
    (next) => { searchState = next; },
    (fn, ms) => setTimeout(fn, ms),
    (id) => clearTimeout(id),
    store.session?.token ?? null,
    initialKind,
  );

  let canRequest = $derived(store.me?.canRequest === true);
  let tabs = $derived([
    { id: 'accounts', label: 'Accounts' },
    { id: 'books', label: 'Books' },
    { id: 'posts', label: 'Posts' },
    ...(canRequest ? [{ id: 'catalog', label: 'Catalog' }] : []),
  ]);

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
      if (text.trim() !== searchState.query.trim()) overrides = {};
      model.setQuery(text);
    });
  });

  $effect(() => {
    const can = canRequest;
    const remembered = saved;
    untrack(() => {
      if (store.me == null || restoredCatalog) return;
      restoredCatalog = true;
      if (remembered === 'catalog' && can && searchState.kind !== 'catalog') model.setKind('catalog');
      if (!can && searchState.kind === 'catalog') model.setKind('accounts');
    });
  });

  function select(id: string): void {
    if (id !== 'accounts' && id !== 'books' && id !== 'posts' && id !== 'catalog') return;
    if (id === 'catalog' && !canRequest) return;
    writeStored('mewo.search.kind', id);
    overrides = {};
    model.setKind(id);
  }

  function shown(work: CatalogWork): BookRequest | null {
    return overrides[work.workKey] ?? work.request;
  }

  async function act(work: CatalogWork): Promise<void> {
    const current = shown(work);
    const action = rowAction(current);
    try {
      if (action === 'request') {
        overrides[work.workKey] = await store.api.requestBook(work);
      } else if (action === 'retry' && current) {
        overrides[work.workKey] = await store.api.retryRequest(current.id);
      } else if (action === 'open' && current?.bookId) {
        router.go(readerUrl(current.bookId));
      }
    } catch (error) {
      store.notice = message(error);
    }
  }

  let page = $derived(visiblePage(searchState));
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

<UnderlineTabs {tabs} selected={searchState.kind} fixed onselect={select} />

{#if searchState.phase.status === 'loading'}
  <div class="status-row" role="status">
    <span class="spinner"></span>
    <span class="sr-only">Searching…</span>
  </div>
{/if}
{#if searchState.phase.status === 'failed'}
  <div class="status-row">
    <span class="error" role="alert">{searchState.phase.error}</span>
    <button class="text-button" onclick={() => model.retry()}>Try again</button>
  </div>
{/if}
{#if page?.type === 'accounts'}
  {#each page.items as account (account.username)}
    <button class="account-row" onclick={() => onprofile(account.username)}>
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
    {@const current = shown(work)}
    {@const action = rowAction(current)}
    <div class="book-row">
      <div class="book-link">
        <BookCover title={work.title} src={coverUrl(work.coverId)} />
        <div class="post-column">
          <div class="post-meta">
            <strong class="truncate">{work.title}</strong>
          </div>
          <p class="meta">{work.author}{work.year != null ? ` · ${work.year}` : ''}</p>
        </div>
      </div>
      <div class="book-actions">
        <button
          class="icon-button"
          aria-label={action === 'request' ? 'Request' : action === 'pending' && current ? stateLabel(current) : action === 'open' ? 'On your shelf' : 'Retry'}
          disabled={action === 'pending'}
          onclick={() => { void act(work); }}
        >
          <Icon
            name={action === 'request' ? 'plus' : action === 'pending' ? 'clock' : action === 'open' ? 'check' : 'retry'}
            size={18}
          />
        </button>
      </div>
    </div>
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
{#if showNoMatches(searchState)}
  <div class="empty-state">
    <h2>No matches</h2>
  </div>
{/if}

<style>
  .book-row {
    display: flex;
    align-items: flex-start;
    border-bottom: .6px solid var(--line);
    padding-right: 12px;
  }

  .book-row:hover {
    background: var(--hover);
  }

  .book-link {
    flex: 1;
    min-width: 0;
    display: flex;
    gap: 12px;
    padding: 12px 4px 16px 16px;
  }

  .book-link p {
    margin-top: 3px;
    font-size: .9375rem;
    line-height: 1.33;
  }

  .book-row .post-meta {
    height: 21px;
  }

  .book-actions {
    display: flex;
    align-items: center;
    flex: 0 0 auto;
    margin-top: 12px;
    height: 21px;
    min-height: 0;
  }

  .book-actions > .icon-button {
    width: 40px;
    height: 40px;
    padding: 0;
    line-height: 0;
    color: var(--mute);
  }
</style>
