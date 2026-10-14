<script lang="ts">
  import { onMount } from 'svelte';
  import BookRow from '../components/BookRow.svelte';
  import { isFinished } from '../lib/models';
  import type { BookRecord } from '../lib/models';
  import { restoreScroll } from '../lib/screen';
  import { store } from '../lib/store.svelte';

  let { scroller, uploading, onadd, onsample, onremove }: {
    scroller: HTMLElement;
    uploading: boolean;
    onadd: () => void;
    onsample: () => void;
    onremove: (book: BookRecord) => void;
  } = $props();

  let activeBooks = $derived(store.books.filter((book) => !isFinished(book)));
  let finishedBooks = $derived(store.books.filter(isFinished));
  onMount(() => restoreScroll(scroller));
</script>

{#if !store.books.length}
  <div class="empty-state shelf-empty">
    <h2>Your timeline is empty</h2>
    <p class="muted">
      Drop in an EPUB and the author starts posting, one paragraph at a time.
    </p>
    <button class="pill" disabled={uploading} onclick={onadd}>Add a book</button>
    <button class="text-button" disabled={uploading} onclick={onsample}>
      Open a sample
    </button>
  </div>
{:else}
  {#each activeBooks as book (book.id)}
    <BookRow {book} {onremove} />
  {/each}
  {#if finishedBooks.length}
    <h2 class="section-label">Finished</h2>
    {#each finishedBooks as book (book.id)}
      <BookRow {book} {onremove} />
    {/each}
  {/if}
{/if}
