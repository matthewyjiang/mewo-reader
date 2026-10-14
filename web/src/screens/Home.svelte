<script lang="ts">
  import { onMount } from 'svelte';
  import BookRow from '../components/BookRow.svelte';
  import { shelfGroups } from '../lib/models';
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

  let groups = $derived(shelfGroups(store.books));
  onMount(() => restoreScroll(scroller));
</script>

{#snippet rows(books: BookRecord[])}
  {#each books as book (book.id)}
    <BookRow {book} {onremove} />
  {/each}
{/snippet}

{#if !store.books.length}
  <div class="empty-state shelf-empty">
    <h2>No books</h2>
    <button class="pill" disabled={uploading} onclick={onadd}>Add a book</button>
    <button class="text-button" disabled={uploading} onclick={onsample}>
      Open a sample
    </button>
  </div>
{:else}
  {#if groups.mine.length}
    <h2 class="section-label">My books</h2>
    {@render rows(groups.mine)}
  {/if}
  {#if groups.sharedWithMe.length}
    <h2 class="section-label">Shared with me</h2>
    {@render rows(groups.sharedWithMe)}
  {/if}
  {#if groups.finished.length}
    <h2 class="section-label">Finished</h2>
    {@render rows(groups.finished)}
  {/if}
{/if}
