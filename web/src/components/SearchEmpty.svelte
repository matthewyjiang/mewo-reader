<script lang="ts">
  import type { SearchKind } from '../lib/models';
  import { homeUrl, router } from '../lib/router.svelte';

  let { kind, mode, query, canRequest, isAdmin, onkind, onadd }: {
    kind: SearchKind;
    mode: 'noMatches' | 'nothingYet';
    query: string;
    canRequest: boolean;
    isAdmin: boolean;
    onkind: (kind: SearchKind) => void;
    onadd: () => void;
  } = $props();

  let quoted = $derived(`“${query.trim()}”`);
</script>

<div class="empty-state" class:catalog={kind === 'catalog'}>
  {#if kind === 'accounts' && mode === 'noMatches'}
    <h2>No accounts match {quoted}</h2>
  {:else if kind === 'accounts'}
    <h2>It's just you here so far.</h2>
    {#if isAdmin}
      <p>
        Add people with <code>mewo-server admin create &lt;name&gt;</code>
        or turn on sign-ups in Settings › Server.
      </p>
    {/if}
  {:else if kind === 'books' && mode === 'noMatches'}
    <h2>No books match {quoted}</h2>
    {#if canRequest}
      <button class="pill" type="button" onclick={() => onkind('catalog')}>Search the Catalog</button>
    {/if}
  {:else if kind === 'books'}
    <h2>No books yet.</h2>
    <button class="pill" type="button" onclick={onadd}>Add an EPUB</button>
    {#if canRequest}
      <button class="text-button" type="button" onclick={() => onkind('catalog')}>Browse the Catalog</button>
    {/if}
  {:else if kind === 'posts' && mode === 'noMatches'}
    <h2>No posts match {quoted}</h2>
  {:else if kind === 'posts'}
    <h2>No posts yet. Replies and reposts on books show up here.</h2>
    <button class="pill" type="button" onclick={() => router.go(homeUrl('books'))}>Go to your books</button>
  {:else if mode === 'noMatches'}
    <h2>Nothing in Open Library matches {quoted}. Try the author's name or fewer words.</h2>
  {:else}
    <h2>No trending books in your languages right now. Search above.</h2>
  {/if}
</div>

<style>
  code {
    font-family: ui-monospace, monospace;
    font-size: .9em;
  }

  .catalog h2 {
    font-size: 1.375rem;
    line-height: 26px;
  }
</style>
