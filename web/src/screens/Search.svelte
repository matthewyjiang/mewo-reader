<script lang="ts">
  import { onMount } from 'svelte';
  import BookRow from '../components/BookRow.svelte';
  import Post from '../components/Post.svelte';
  import { readerUrl, router } from '../lib/router.svelte';
  import { restoreScroll } from '../lib/screen';
  import type { FeedScreenProps } from '../lib/screen';
  import { store } from '../lib/store.svelte';

  let { scroller, query, oncomment }: FeedScreenProps & { query: string } = $props();
  let results = $derived(store.search(query));
  onMount(() => restoreScroll(scroller));
</script>

{#if !query.trim()}
  <div class="empty-state">
    <h2>Find a line</h2>
    <p class="muted">Search your shelf and posts from books loaded this session.</p>
  </div>
{:else if !results.books.length && !results.posts.length}
  <div class="empty-state">
    <h2>No matches</h2>
    <p class="muted">Try another word. Open a book to include its posts in search.</p>
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
{/if}
