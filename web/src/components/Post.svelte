<script lang="ts">
  import Avatar from './Avatar.svelte';
  import Icon from './Icon.svelte';
  import type { BookRecord, FeedPost } from '../lib/models';
  import { store, message } from '../lib/store.svelte';
  import { readerUrl, router } from '../lib/router.svelte';
  import { hasReposted, repostCount } from '../lib/repost';
  import { shareText } from '../lib/share';

  let { book, post, oncomment, onopen, onchapter, connected = false }: {
    book: BookRecord;
    post: FeedPost;
    oncomment: () => void;
    onopen?: () => void;
    onchapter?: () => void;
    connected?: boolean;
  } = $props();

  let liking = $state(false);
  let reposting = $state(false);
  let liked = $derived((store.likes[book.id] ?? []).includes(post.id));
  let count = $derived(store.indexes[book.id]?.counts[post.id] ?? 0);
  let commented = $derived((store.indexes[book.id]?.mine ?? []).includes(post.id));
  let reposted = $derived(hasReposted(store.indexes[book.id], post.id));
  let reposts = $derived(repostCount(store.indexes[book.id], post.id));

  async function toggle(): Promise<void> {
    if (liking) return;
    liking = true;
    try {
      await store.toggleLike(book.id, post.id);
    } catch (error) {
      store.notice = message(error);
    } finally {
      liking = false;
    }
  }

  async function repost(): Promise<void> {
    if (reposting) return;
    reposting = true;
    try {
      await store.toggleRepost(book.id, post.id);
    } catch (error) {
      store.notice = message(error);
    } finally {
      reposting = false;
    }
  }

  async function share(quote: boolean): Promise<void> {
    try {
      const text = quote ? `${post.text}\n\n— ${book.author}, ${book.title}` : post.text;
      if (await shareText(text)) store.notice = 'Copied to clipboard.';
    } catch (error) {
      store.notice = message(error);
    }
  }
</script>

<article class:connected class="post" aria-label={`${book.author}, ${post.chapter}`}>
  <div class="avatar-column">
    <Avatar name={book.author} src={store.covers[book.id]} />
    {#if connected}<span class="thread-rule"></span>{/if}
  </div>
  <div class="post-column">
    <div class="post-meta">
      <strong class="post-name">{book.author}</strong>
      {#if onchapter}
        <button
          class="chapter meta"
          onclick={onchapter}
          aria-label={`Chapters, ${post.chapter}`}
        >
          @{book.handle} · {post.chapter}
        </button>
      {:else}
        <span class="meta truncate">@{book.handle} · {post.chapter}</span>
      {/if}
    </div>
    {#if onopen}
      <a
        class:heading={post.kind === 'Heading'}
        class="post-body post-link"
        href={readerUrl(book.id, post.id)}
        onclick={(event) => { event.preventDefault(); onopen?.(); }}
      >
        {post.text}
      </a>
    {:else}
      <div class:heading={post.kind === 'Heading'} class="post-body">{post.text}</div>
    {/if}
    <div class="post-actions">
      <button
        class:accent={commented}
        class="icon-button"
        aria-label={count ? `Comments, ${count}` : 'Comment on this line'}
        onclick={oncomment}
      >
        <Icon name="comment" size={18} />
        {#if count}<span class="meta">{count}</span>{/if}
      </button>
      <button
        class:reposted
        class="icon-button repost-action"
        aria-label={reposts ? `Reposts, ${reposts}` : reposted ? 'Undo repost' : 'Repost'}
        aria-pressed={reposted}
        disabled={reposting}
        onclick={repost}
      >
        <Icon name="repost" size={18} />
        {#if reposts}<span class="meta">{reposts}</span>{/if}
      </button>
      <button
        class="icon-button quote-action"
        aria-label="Quote this line"
        onclick={() => share(true)}
      >
        <Icon name="quote" size={18} />
      </button>
      <button
        class:liked
        class="icon-button like-action"
        aria-label={liked ? 'Unlike' : 'Like'}
        aria-pressed={liked}
        disabled={liking}
        onclick={toggle}
      >
        <Icon name="heart" filled={liked} size={18} />
      </button>
      <button class="icon-button" aria-label="Share" onclick={() => share(false)}>
        <Icon name="share" size={18} />
      </button>
    </div>
    {#if connected}
      <button
        class="text-button meta"
        onclick={() => router.go(readerUrl(book.id, post.id))}
      >
        Open line in book
      </button>
    {/if}
  </div>
</article>

<style>
.chapter {
  text-align: left;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  padding-block: 4px;
}

.chapter:hover {
  text-decoration: underline;
}

.post-link {
  display: block;
}

.post-link:hover {
  text-decoration: none;
}

.post-actions {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 4px;
  margin-top: 4px;
  color: var(--mute);
}

.post-actions .liked {
  color: var(--like);
}

.post-actions .reposted {
  color: var(--green);
}

.repost-action:hover:not(:disabled) {
  color: var(--green);
  background: color-mix(in srgb, var(--green) 10%, transparent);
}

.like-action:hover:not(:disabled) {
  color: var(--like);
  background: color-mix(in srgb, var(--like) 10%, transparent);
}

.quote-action:hover:not(:disabled) {
  color: var(--green);
  background: color-mix(in srgb, var(--green) 10%, transparent);
}

.like-action[aria-pressed='true'] :global(svg) {
  transform: scale(1.12);
}

.like-action :global(svg) {
  transition: transform 200ms ease-out;
}



</style>
