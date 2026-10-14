<script lang="ts">
  import { onMount, tick } from 'svelte';
  import FeedStatus from '../components/FeedStatus.svelte';
  import ChapterSheet from '../components/ChapterSheet.svelte';
  import Post from '../components/Post.svelte';
  import Timeline from '../components/Timeline.svelte';
  import type { TimelineAnchor } from '../components/Timeline.svelte';
  import { chrome } from '../lib/chrome.svelte';
  import { currentHeadingIndex } from '../lib/models';
  import type { BookRecord, FeedPost } from '../lib/models';
  import { router } from '../lib/router.svelte';
  import type { FeedScreenProps } from '../lib/screen';
  import { store, message } from '../lib/store.svelte';

  let { id, focusPostId, scroller, oncomment, onloaded, onbusy }: FeedScreenProps & {
    id: string;
    focusPostId: string | null;
    onloaded: (book: BookRecord) => void;
    onbusy: (busy: boolean) => void;
  } = $props();

  let book = $state<BookRecord | null>(null);
  let posts = $state.raw<FeedPost[]>([]);
  let loading = $state(true);
  let error = $state('');
  let chapters = $state(false);
  let visibleIndex = $state(0);
  let headings = $derived(
    posts.flatMap((post, index) => post.kind === 'Heading' ? [{ post, index }] : []),
  );
  let currentChapter = $derived(currentHeadingIndex(headings, visibleIndex));
  let active = true;
  let ready = false;
  let timeline = $state<ReturnType<typeof Timeline>>();
  const entryKey = router.current.key;

  export function openChapters(): void {
    if (!loading && !error) chapters = true;
  }

  async function load(): Promise<void> {
    loading = true;
    error = '';
    ready = false;
    onbusy(true);
    chrome.show();
    const token = store.session?.token;
    try {
      const nextBook = await store.book(id);
      if (!active || token !== store.session?.token) return;
      const [feed] = await Promise.all([store.feed(id), store.social(id)]);
      if (!active || token !== store.session?.token) return;
      book = nextBook;
      posts = feed;
      onloaded(nextBook);
      const focus = focusPostId === null ? -1 : feed.findIndex((post) => post.id === focusPostId);
      visibleIndex = focus >= 0
        ? focus
        : Math.min(Math.max(0, nextBook.progressIndex), feed.length);
      loading = false;
      await tick();
      if (!active) return;
      const saved = router.scroll.get(entryKey);
      if (saved && typeof saved !== 'number') await jump(saved.index, false, saved.offset);
      else if (posts.length) await jump(visibleIndex, false);
      else scroller.scrollTop = 0;
      chrome.reset(scroller.scrollTop);
      scroller.focus({ preventScroll: true });
      ready = true;
    } catch (err) {
      if (active) { error = message(err); loading = false; }
    } finally {
      if (active) onbusy(false);
    }
  }

  function track(anchor: TimelineAnchor): void {
    if (!ready || !active) return;
    visibleIndex = anchor.index;
    store.saveProgress(id, anchor.index);
  }

  export function capture(): void {
    if (!ready || !timeline) return;
    const anchor = timeline.anchor();
    track(anchor);
    router.scroll.set(entryKey, anchor);
  }

  async function jump(index: number, save = true, offset = 0): Promise<void> {
    if (!timeline) return;
    ready = false;
    chrome.suspended = true;
    chrome.show();
    await tick();
    await timeline.jump(index, offset);
    if (!active) { chrome.suspended = false; return; }
    visibleIndex = index;
    if (save) store.saveProgress(id, index);
    chapters = false;
    chrome.reset(scroller.scrollTop);
    chrome.suspended = false;
    ready = true;
  }

  onMount(() => {
    void load();
    return () => {
      capture();
      active = false;
      ready = false;
      chrome.suspended = false;
      store.flushProgress(id);
    };
  });
</script>

{#if loading || error}
  <FeedStatus {loading} {error} label="your timeline" onretry={load} />
{:else if book}
  <Timeline
    bind:this={timeline}
    count={posts.length + 1}
    {scroller}
    inset={chrome.shown ? 56 : 0}
    onvisible={track}
    onadjust={(y) => chrome.rebase(y)}
  >
    {#snippet row(index: number)}
      {#if index < posts.length}
        <Post
          book={book!}
          post={posts[index]}
          oncomment={() => oncomment(book!, posts[index])}
          onchapter={openChapters}
        />
      {:else}
        <div class="end-card">
          <p>{posts.length ? `End of ${book!.title}` : 'This book has no readable posts.'}</p>
          <button class="text-button" onclick={() => router.go('/')}>Back to your shelf</button>
        </div>
      {/if}
    {/snippet}
  </Timeline>
{/if}

{#if chapters && book}
  <ChapterSheet
    title={book.title}
    {headings}
    current={currentChapter}
    onjump={(index) => { void jump(index); }}
    onclose={() => chapters = false}
  />
{/if}

<style>
.end-card {
  /* Allow the end card itself to reach the top even with hidden navigation. */
  min-height: calc(100dvh - env(safe-area-inset-top) - env(safe-area-inset-bottom));
  text-align: center;
  padding: 48px 24px;
  color: var(--mute);
  scroll-margin-top: 0;
}

.end-card .text-button {
  text-align: center;
  margin-top: 16px;
}

</style>
