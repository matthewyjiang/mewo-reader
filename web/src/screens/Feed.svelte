<script lang="ts">
  import { onMount } from 'svelte';
  import Post from '../components/Post.svelte';
  import ThreadRow from '../components/ThreadRow.svelte';
  import Icon from '../components/Icon.svelte';
  import { feedMoreAllowed, showFeedEmpty } from '../lib/feed';
  import { relativeTime } from '../lib/models';
  import type { BookRecord, FeedPost } from '../lib/models';
  import { readerUrl, router } from '../lib/router.svelte';
  import { restoreScroll } from '../lib/screen';
  import { store } from '../lib/store.svelte';

  let { scroller, oncomment, onprofile }: {
    scroller: HTMLElement;
    oncomment: (book: BookRecord, post: FeedPost) => void;
    onprofile: (username: string) => void;
  } = $props();

  onMount(() => restoreScroll(scroller));

  let phase = $derived(store.home);
  let page = $derived(phase.status === 'loading' ? null : phase.page);
  let socialIds = $derived(page ? [...new Set(page.items.map((item) => item.book.id))] : []);
  let socialKey = $derived(JSON.stringify(socialIds));
  $effect(() => {
    socialKey;
    for (const id of socialIds) void store.social(id).catch(() => {});
  });

  function sentinel(node: HTMLElement): { destroy: () => void } {
    const observer = new IntersectionObserver((entries) => {
      if (entries.some((entry) => entry.isIntersecting)) void store.moreFeed();
    }, { root: scroller });
    observer.observe(node);
    return { destroy: () => observer.disconnect() };
  }
</script>

{#if phase.status === 'loading'}
  <div class="status-row" role="status">
    <span class="spinner"></span>
    <span class="sr-only">Loading feed…</span>
  </div>
{/if}
{#if phase.status === 'failed'}
  <div class="status-row">
    <span class="error" role="alert">{phase.error}</span>
    <button class="text-button" onclick={() => store.loadFeed()}>Try again</button>
  </div>
{/if}
{#if page}
  {#each page.items as item (`${item.type}:${item.at}:${item.type === 'comment' ? item.comment.id : item.actor.username}:${item.book.id}:${item.post.id}`)}
    {#if item.type === 'repost'}
      <button class="activity-line" onclick={() => onprofile(item.actor.username)}>
        <Icon name="repost" size={16} />
        <span>{item.actor.username} reposted</span>
      </button>
      <Post
        book={item.book}
        post={item.post}
        onopen={() => router.go(readerUrl(item.book.id, item.post.id))}
        oncomment={() => oncomment(item.book, item.post)}
      />
    {:else}
      <div class="profile-reply">
        <Post
          book={item.book}
          post={item.post}
          connected
          onopen={() => router.go(readerUrl(item.book.id, item.post.id))}
          oncomment={() => oncomment(item.book, item.post)}
        />
        <ThreadRow
          name={item.actor.username}
          handle={item.actor.username}
          meta={relativeTime(item.comment.createdAt)}
          text={item.comment.text}
          {onprofile}
          onopen={() => oncomment(item.book, item.post)}
        />
      </div>
    {/if}
  {/each}
{/if}
{#if phase.status === 'loaded' && phase.more}
  <div class="status-row" role="status">
    <span class="spinner"></span>
  </div>
{/if}
{#if feedMoreAllowed(phase)}
  <div use:sentinel></div>
{/if}
{#if showFeedEmpty(phase)}
  <div class="empty-state">
    <h2>Nothing here yet</h2>
  </div>
{/if}

<style>
.profile-reply {
  border-bottom: .6px solid var(--line);
}

.profile-reply :global(.post) {
  border-bottom: 0;
}

.profile-reply :global(.thread-row) {
  padding-bottom: 16px;
}
</style>
