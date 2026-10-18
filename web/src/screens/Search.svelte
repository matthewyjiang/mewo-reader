<script lang="ts">
  import { onMount, untrack } from 'svelte';
  import BookRow from '../components/BookRow.svelte';
  import Post from '../components/Post.svelte';
  import UnderlineTabs from '../components/UnderlineTabs.svelte';
  import Avatar from '../components/Avatar.svelte';
  import type { SearchKind } from '../lib/models';
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
  const initialKind: SearchKind = saved === 'books' || saved === 'posts' ? saved : 'accounts';
  const entryKey = untrack(() => router.current.key);
  let state = $state<SearchState>({
    query: '',
    kind: initialKind,
    phase: { status: 'idle' },
    session: store.session?.token ?? null,
  });
  const model = new SearchModel(
    (request) => store.api.search(request.query, request.kind, request.cursor, request.signal),
    (next) => { state = next; },
    (fn, ms) => setTimeout(fn, ms),
    (id) => clearTimeout(id),
    store.session?.token ?? null,
    initialKind,
  );

  const tabs = [
    { id: 'accounts', label: 'Accounts' },
    { id: 'books', label: 'Books' },
    { id: 'posts', label: 'Posts' },
  ];

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
    untrack(() => model.setQuery(text));
  });

  function select(id: string): void {
    if (id !== 'accounts' && id !== 'books' && id !== 'posts') return;
    writeStored('mewo.search.kind', id);
    model.setKind(id);
  }

  let page = $derived(visiblePage(state));
  let socialIds = $derived(page?.type === 'posts' ? [...new Set(page.items.map((hit) => hit.book.id))] : []);
  let socialKey = $derived(JSON.stringify(socialIds));
  $effect(() => {
    socialKey;
    untrack(() => {
      void Promise.all(socialIds.map((id) => store.social(id)));
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

<UnderlineTabs {tabs} selected={state.kind} fixed onselect={select} />

{#if state.phase.status === 'loading'}
  <div class="status-row" role="status">
    <span class="spinner"></span>
    <span class="sr-only">Searching…</span>
  </div>
{/if}
{#if state.phase.status === 'failed'}
  <div class="status-row">
    <span class="error" role="alert">{state.phase.error}</span>
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
{#if state.phase.status === 'loaded' && state.phase.more}
  <div class="status-row" role="status">
    <span class="spinner"></span>
    <span class="sr-only">Loading more…</span>
  </div>
{/if}
{#if state.phase.status === 'loaded' && state.phase.page.nextCursor && !state.phase.more}
  <div use:sentinel></div>
{/if}
{#if showNoMatches(state)}
  <div class="empty-state">
    <h2>No matches</h2>
  </div>
{/if}
