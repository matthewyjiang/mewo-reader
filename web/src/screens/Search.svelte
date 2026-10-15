<script lang="ts">
  import { onMount, tick, untrack } from 'svelte';
  import BookRow from '../components/BookRow.svelte';
  import Post from '../components/Post.svelte';
  import Timeline from '../components/Timeline.svelte';
  import { chrome } from '../lib/chrome.svelte';
  import type { BookRecord, PostHit } from '../lib/models';
  import { readerUrl, router } from '../lib/router.svelte';
  import { restoreScroll } from '../lib/screen';
  import type { FeedScreenProps } from '../lib/screen';
  import { searchLibrary } from '../lib/search';
  import type { SearchResults } from '../lib/search';
  import { store, message } from '../lib/store.svelte';

  type Row = { key: string } & ({ kind: 'heading'; title: string }
    | { kind: 'book'; book: BookRecord } | { kind: 'post'; hit: PostHit });
  const empty = { query: '', books: [], posts: [] };
  let { scroller, query, oncomment }: FeedScreenProps & { query: string } = $props();
  let results = $state.raw<SearchResults & { query: string }>(empty);
  let pending = $state(false);
  let timeline = $state<ReturnType<typeof Timeline>>();
  const entryKey = untrack(() => router.current.key);
  const initialQuery = untrack(() => query);
  let initialScroll = router.scroll.get(entryKey);

  export function capture(): void {
    const anchor = timeline && query.trim() && query === results.query ? timeline.anchor() : 0;
    router.scroll.set(entryKey, anchor);
    router.tabScroll.set('search', anchor);
  }
  onMount(() => restoreScroll(scroller));
  let rows = $derived<Row[]>([
    ...(results.books.length ? [{ key: 'books', kind: 'heading' as const, title: 'Books' },
      ...results.books.map((book) => ({ key: JSON.stringify(['book', book.id]), kind: 'book' as const, book }))] : []),
    ...(results.posts.length ? [{ key: 'posts', kind: 'heading' as const, title: 'Posts' },
      ...results.posts.map((hit) => ({ key: JSON.stringify(['post', hit.book.id, hit.post.id]), kind: 'post' as const, hit }))] : []),
  ]);
  let keys = $derived(rows.map((row) => row.key));

  $effect(() => {
    const text = query;
    const books = [...store.books];
    const feeds = store.feeds;
    const controller = new AbortController();
    pending = !!text.trim();
    if (!text.trim()) {
      results = empty;
      initialScroll = undefined;
      scroller.scrollTop = 0;
      chrome.reset();
    } else {
      void searchLibrary(books, feeds, text, controller.signal).then(async (next) => {
        if (!next || controller.signal.aborted) return;
        const newQuery = text !== results.query;
        const saved = text === initialQuery ? initialScroll : undefined;
        results = { query: text, ...next };
        pending = false;
        await tick();
        if (controller.signal.aborted) return;
        if (saved && typeof saved !== 'number') await timeline?.restore(saved);
        else if (newQuery) await timeline?.jump(0);
        initialScroll = undefined;
        if (!controller.signal.aborted) chrome.reset(scroller.scrollTop);
      });
    }
    return () => controller.abort();
  });

  // Changing the query should not refetch the same books' likes and comments.
  let socialIds = $derived([...new Set(results.posts.map((hit) => hit.book.id))]);
  let socialKey = $derived(JSON.stringify(socialIds));
  $effect(() => {
    socialKey;
    untrack(() => {
      void Promise.all(socialIds.map((id) => store.social(id)))
        .catch((error: unknown) => store.notice = message(error));
    });
  });
</script>

<div aria-busy={pending}>
  {#if pending}<span class="sr-only" role="status">Searching…</span>{/if}
  {#if query.trim() && !pending && !rows.length}
    <div class="empty-state">
      <h2>No matches</h2>
    </div>
  {:else if query.trim() && rows.length}
    {#key results.query}
      <Timeline bind:this={timeline} {keys} {scroller} inset={56} onadjust={(y) => chrome.rebase(y)}>
        {#snippet row(index: number)}
          {@const item = rows[index]}
          {#if item.kind === 'heading'}
            <h2 class="section-label">{item.title}</h2>
          {:else if item.kind === 'book'}
            <BookRow book={item.book} />
          {:else}
            <Post
              book={item.hit.book}
              post={item.hit.post}
              onopen={() => router.go(readerUrl(item.hit.book.id, item.hit.post.id))}
              oncomment={() => oncomment(item.hit.book, item.hit.post)}
            />
          {/if}
        {/snippet}
      </Timeline>
    {/key}
  {/if}
</div>
