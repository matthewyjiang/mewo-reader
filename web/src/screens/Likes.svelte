<script lang="ts">
  import { onMount, tick } from 'svelte';
  import FeedStatus from '../components/FeedStatus.svelte';
  import Post from '../components/Post.svelte';
  import { chrome } from '../lib/chrome.svelte';
  import { router, readerUrl } from '../lib/router.svelte';
  import { restoreScroll } from '../lib/screen';
  import type { FeedScreenProps } from '../lib/screen';
  import { store, message } from '../lib/store.svelte';

  let { scroller, oncomment }: FeedScreenProps = $props();
  let loading = $state(true);
  let error = $state('');
  let hits = $derived(store.likedPosts());
  let active = true;

  async function load(): Promise<void> {
    loading = true;
    error = '';
    chrome.show();
    try {
      await store.loadLikes();
      if (!active) return;
      loading = false;
      await tick();
      if (active) restoreScroll(scroller);
    } catch (err) {
      if (active) { error = message(err); loading = false; }
    }
  }

  onMount(() => {
    void load();
    return () => { active = false; };
  });
</script>

{#if loading || error}
  <FeedStatus {loading} {error} label="your likes" onretry={load} />
{:else if !hits.length}
  <div class="empty-state">
    <h2>No likes yet</h2>
    <p class="muted">Heart a line. It stays here, across all your books.</p>
  </div>
{:else}
  {#each hits as hit (`${hit.book.id}:${hit.post.id}`)}
    <div class="timeline-item">
      <Post
        book={hit.book}
        post={hit.post}
        onopen={() => router.go(readerUrl(hit.book.id, hit.post.id))}
        oncomment={() => oncomment(hit.book, hit.post)}
      />
    </div>
  {/each}
{/if}
