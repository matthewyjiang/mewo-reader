<script lang="ts">
  import type { BookRecord } from '../lib/models';
  import { statusLine } from '../lib/models';
  import { store } from '../lib/store.svelte';
  import { router, readerUrl } from '../lib/router.svelte';
  import Avatar from './Avatar.svelte';
  import Icon from './Icon.svelte';

  let { book, onremove }: {
    book: BookRecord;
    onremove?: (book: BookRecord) => void;
  } = $props();
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
    </div>
  </a>
  {#if book.mine && onremove}
    <button
      class="icon-button"
      aria-label={`Remove ${book.title}`}
      onclick={() => onremove?.(book)}
    >
      <Icon name="trash" size={18} />
    </button>
  {/if}
</div>
