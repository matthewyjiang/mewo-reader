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
      <label class="share-switch">
        <span>{book.shared ? 'Shared' : 'Share'}</span>
        <input
          type="checkbox"
          role="switch"
          checked={book.shared}
          aria-label={`${book.shared ? 'Stop sharing' : 'Share'} ${book.title}`}
          onchange={(event) => { void share(event.currentTarget.checked); }}
        />
      </label>
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

.book-actions {
  display: flex;
  align-items: center;
  flex: 0 0 auto;
  margin-top: 6px;
}

.book-actions > .icon-button {
  color: var(--mute);
}

.share-switch {
  display: flex;
  align-items: center;
  gap: 6px;
  min-height: 40px;
  padding-inline: 4px;
  color: var(--mute);
  font-size: .8125rem;
  cursor: pointer;
}

.share-switch input {
  width: 36px;
  height: 24px;
  margin: 0;
  cursor: pointer;
}

.book-row .post-meta > strong {
  max-width: 70%;
  font-size: 1.0625rem;
}



</style>
