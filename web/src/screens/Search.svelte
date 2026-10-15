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

  type Row = { kind: 'heading'; title: string }
    | { kind: 'book'; book: BookRecord } | { kind: 'post'; hit: PostHit };
  const empty: SearchResults = { books: [], posts: [] };
  let { scroller, query, oncomment }: FeedScreenProps & { query: string } = $props();
  let results = $state.raw<SearchResults>(empty);
  let pending = $state(false);
  let firstResults = true;
  let resultQuery = '';
  let timeline = $state<ReturnType<typeof Timeline>>();
  const entryKey = untrack(() => router.current.key);
  const savedScroll = router.scroll.get(entryKey);

  export function capture(): void {
    const anchor = timeline && query.trim() && query === resultQuery ? timeline.anchor() : 0;
    router.scroll.set(entryKey, anchor);
    router.tabScroll.set('search', anchor);
  }
  onMount(() => restoreScroll(scroller));
  let rows = $derived<Row[]>([
    ...(results.books.length ? [{ kind: 'heading' as const, title: 'Books' },
      ...results.books.map((book) => ({ kind: 'book' as const, book }))] : []),
    ...(results.posts.length ? [{ kind: 'heading' as const, title: 'Posts' },
      ...results.posts.map((hit) => ({ kind: 'post' as const, hit }))] : []),
  ]);

  $effect(() => {
    const text = query;
    // Track shelf/feed changes as well as typing, but not social state.
    const books = [...store.books];
    const feeds = store.feeds;
    const controller = new AbortController();
    pending = !!text.trim();
    if (!text.trim()) {
      results = empty;
      scroller.scrollTop = 0;
      chrome.reset();
    } else {
      void searchLibrary(books, feeds, text, controller.signal).then(async (next) => {
        if (!next || controller.signal.aborted) return;
        // Pixel offsets change when virtual rows are measured again. Keep the
        // visible row on return or a shelf refresh; a new query starts at the top.
        const restore = firstResults ? savedScroll
          : text === resultQuery ? timeline?.anchor() : 0;
        results = next;
        resultQuery = text;
        pending = false;
        firstResults = false;
        await tick();
        if (controller.signal.aborted) return;
        if (restore && typeof restore !== 'number') {
          await timeline?.jump(restore.index, restore.offset);
        } else scroller.scrollTop = restore ?? 0;
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
    {#key results}
      <Timeline bind:this={timeline} count={rows.length} {scroller} inset={56} onadjust={(y) => chrome.rebase(y)}>
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
