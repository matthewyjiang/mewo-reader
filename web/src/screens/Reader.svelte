<script lang="ts">
  import { onMount, tick } from 'svelte';
  import FeedStatus from '../components/FeedStatus.svelte';
  import Icon from '../components/Icon.svelte';
  import Post from '../components/Post.svelte';
  import Sheet from '../components/Sheet.svelte';
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
  let frame = 0;
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
    try {
      const nextBook = await store.book(id);
      const [feed] = await Promise.all([store.feed(id), store.social(id)]);
      if (!active) return;
      book = nextBook;
      posts = feed;
      onloaded(nextBook);
      const focus = focusPostId === null ? -1 : feed.findIndex((post) => post.id === focusPostId);
      visibleIndex = focus >= 0
        ? focus
        : Math.min(Math.max(0, nextBook.progressIndex), Math.max(0, feed.length - 1));
      loading = false;
      await tick();
      if (!active) return;
      const saved = router.scroll.get(entryKey);
      if (saved !== undefined) scroller.scrollTop = saved;
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

  /** Probe the visible DOM rather than measuring every paragraph in a long book. */
  function track(): void {
    if (frame || !ready) return;
    frame = requestAnimationFrame(() => {
      frame = 0;
      if (!ready || !active || !posts.length) return;
      const box = scroller.getBoundingClientRect();
      // Look beneath the header while its hide/show transition is still in flight.
      const node = document.elementsFromPoint(
        box.left + box.width / 2,
        box.top + (chrome.shown ? 56 : 0) + 1,
      ).map((element) => element.closest<HTMLElement>('[data-post-index]'))
        .find((element) => element && scroller.contains(element));
      if (!node) return;
      const index = Number(node.dataset.postIndex);
      if (Number.isInteger(index) && index >= 0 && index <= posts.length) {
        visibleIndex = index;
        store.saveProgress(id, index);
      }
    });
  }

  async function jump(index: number, save = true): Promise<void> {
    const node = scroller.querySelector<HTMLElement>(`[data-post-index="${index}"]`);
    if (!node) return;
    ready = false;
    chrome.suspended = true;
    chrome.show();
    // Reveal estimated offscreen content before aligning a far-away destination.
    node.style.contentVisibility = 'visible';
    node.scrollIntoView({ block: 'start', behavior: 'instant' });
    await new Promise<void>((resolve) => requestAnimationFrame(() => resolve()));
    if (!active) { chrome.suspended = false; return; }
    node.scrollIntoView({ block: 'start', behavior: 'instant' });
    node.style.contentVisibility = '';
    visibleIndex = index;
    if (save) store.saveProgress(id, index);
    chapters = false;
    chrome.reset(scroller.scrollTop);
    chrome.suspended = false;
    ready = true;
  }

  onMount(() => {
    void load();
    scroller.addEventListener('scroll', track, { passive: true });
    return () => {
      active = false;
      ready = false;
      chrome.suspended = false;
      cancelAnimationFrame(frame);
      scroller.removeEventListener('scroll', track);
      store.flushProgress(id);
    };
  });
</script>

{#if loading || error}
  <FeedStatus {loading} {error} label="your timeline" onretry={load} />
{:else if book}
  {#each posts as post, index (post.id)}
    <div class="timeline-item" data-post-index={index}>
      <Post
        {book}
        {post}
        oncomment={() => oncomment(book!, post)}
        onchapter={openChapters}
      />
    </div>
  {/each}
  <div class="end-card" data-post-index={posts.length}>
    <p>{posts.length ? `End of ${book.title}` : 'This book has no readable posts.'}</p>
    <button class="text-button" onclick={() => router.go('/')}>Back to your shelf</button>
  </div>
{/if}

{#if chapters && book}
  <Sheet label={`Chapters, ${book.title}`} onclose={() => chapters = false}>
    <header class="sheet-bar">
      <h2>{book.title}</h2>
      <button
        class="icon-button"
        aria-label="Close chapters"
        onclick={() => chapters = false}
      >
        <Icon name="close" />
      </button>
    </header>
    <div class="chapter-list">
      {#if !headings.length}
        <p class="inline-status">This book has no chapter headings.</p>
      {:else}
        {#each headings as heading, index (heading.post.id)}
          <button
            class="chapter-row"
            class:current={index === currentChapter}
            aria-current={index === currentChapter ? 'location' : undefined}
            onclick={() => jump(heading.index)}
          >
            {heading.post.text}
          </button>
        {/each}
      {/if}
    </div>
  </Sheet>
{/if}
