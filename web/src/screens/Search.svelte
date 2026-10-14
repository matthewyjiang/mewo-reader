<script lang="ts">
  import { onMount, untrack } from 'svelte';
  import BookRow from '../components/BookRow.svelte';
  import Post from '../components/Post.svelte';
  import { readerUrl, router } from '../lib/router.svelte';
  import { restoreScroll } from '../lib/screen';
  import type { FeedScreenProps } from '../lib/screen';
  import { store, message } from '../lib/store.svelte';

  let { scroller, query, oncomment }: FeedScreenProps & { query: string } = $props();
  let results = $derived(store.search(query));
  $effect(() => {
    const ids = [...new Set(results.posts.map((hit) => hit.book.id))];
    untrack(() => {
      void Promise.all(ids.map((id) => store.social(id)))
        .catch((error: unknown) => store.notice = message(error));
    });
  });
  onMount(() => restoreScroll(scroller));
</script>

{#if query.trim() && !results.books.length && !results.posts.length}
  <div class="empty-state">
    <h2>No matches</h2>
  </div>
{:else}
  {#if results.books.length}
    <h2 class="section-label">Books</h2>
    {#each results.books as book (book.id)}
      <BookRow {book} />
    {/each}
  {/if}
  {#if results.posts.length}
    <h2 class="section-label">Posts</h2>
    {#each results.posts as hit (`${hit.book.id}:${hit.post.id}`)}
      <Post
        book={hit.book}
        post={hit.post}
        onopen={() => router.go(readerUrl(hit.book.id, hit.post.id))}
        oncomment={() => oncomment(hit.book, hit.post)}
      />
    {/each}
  {/if}
{/if}
