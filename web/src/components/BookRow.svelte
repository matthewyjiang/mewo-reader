<script lang="ts">
  import type { BookRecord } from '../lib/models';
  import { statusLine } from '../lib/models';
  import { store, message } from '../lib/store.svelte';
  import { router, readerUrl } from '../lib/router.svelte';
  import Avatar from './Avatar.svelte';
  import Icon from './Icon.svelte';

  let { book, onremove }: {
    book: BookRecord;
    onremove?: (book: BookRecord) => void;
  } = $props();

  async function share(shared: boolean): Promise<void> {
    try {
      await store.setShared(book.id, shared);
    } catch (error) {
      store.notice = message(error);
    }
  }
</script>

<div class="book-row">
  <a
    class="book-link"
    href={readerUrl(book.id)}
    onclick={(event) => { event.preventDefault(); router.go(readerUrl(book.id)); }}
  >
    <Avatar name={book.author} src={store.covers[book.id]} />
    <div class="post-column">
      <div class="post-meta">
        <strong class="truncate">{book.title}</strong>
        <span class="meta truncate">@{book.handle}</span>
      </div>
      <p>{statusLine(book)}</p>
      {#if !book.mine && book.ownerUsername}
        <p class="owner">
          {book.shared ? 'Shared' : 'Added'} by @{book.ownerUsername}
        </p>
      {/if}
    </div>
  </a>
  {#if book.mine}
    <div class="book-actions">
      <button
        class="icon-button"
        title={book.shared ? 'Make private' : 'Share'}
        aria-label={book.shared ? 'Make private' : 'Share'}
        aria-pressed={book.shared}
        onclick={() => { void share(!book.shared); }}
      >
        <Icon name={book.shared ? 'people' : 'lock'} filled size={18} />
      </button>
      {#if onremove}
        <button
          class="icon-button"
          aria-label={`Remove ${book.title}`}
          onclick={() => onremove?.(book)}
        >
          <Icon name="trash" size={18} />
        </button>
      {/if}
    </div>
  {/if}
</div>

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

.book-link .owner {
  margin-top: 2px;
  color: var(--mute);
  font-size: .8125rem;
  line-height: 1.23;
}

.book-row .post-meta {
  height: 21px;
}

.book-actions {
  display: flex;
  align-items: center;
  flex: 0 0 auto;
  gap: 0;
  /* 12px matches the link padding. 21px is the title line, so the 40px buttons center on it. */
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

.book-actions > .icon-button :global(svg) {
  display: block;
}

.book-row .post-meta > strong {
  max-width: 70%;
  font-size: 1.0625rem;
  line-height: 21px;
}



</style>
