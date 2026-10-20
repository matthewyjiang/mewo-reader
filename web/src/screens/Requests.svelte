<script lang="ts">
  import { onMount } from 'svelte';
  import Icon from '../components/Icon.svelte';
  import type { BookRequest } from '../lib/models';
  import { stateLabel } from '../lib/requests';
  import { readerUrl, router } from '../lib/router.svelte';
  import { restoreScroll } from '../lib/screen';
  import { message, store } from '../lib/store.svelte';

  let { scroller }: { scroller: HTMLElement } = $props();

  let rows = $state<BookRequest[]>([]);
  let loaded = $state(false);
  let all = $derived(
    router.url.pathname === '/settings/requests' && router.url.searchParams.get('all') === '1',
  );
  const seen = new Map<string, string>();

  async function refresh(announce: boolean): Promise<void> {
    try {
      const next = await store.api.requests(all);
      const arrived = next.some((row) =>
        row.state === 'imported' && seen.has(row.id) && seen.get(row.id) !== 'imported');
      for (const row of next) seen.set(row.id, row.state);
      rows = next;
      loaded = true;
      if (arrived) void store.load();
    } catch (error) {
      if (announce) store.notice = message(error);
    }
  }

  $effect(() => {
    const everyone = all;
    loaded = false;
    rows = [];
    void refresh(true);
    return () => {
      everyone;
    };
  });

  $effect(() => {
    const live = rows.some((row) => row.state === 'requested' || row.state === 'downloading');
    if (!live) return;
    const id = setInterval(() => { void refresh(false); }, 10_000);
    return () => clearInterval(id);
  });

  async function cancel(row: BookRequest): Promise<void> {
    try {
      await store.api.cancelRequest(row.id);
      await refresh(true);
    } catch (error) {
      store.notice = message(error);
    }
  }

  async function retry(row: BookRequest): Promise<void> {
    try {
      const next = await store.api.retryRequest(row.id);
      rows = rows.map((item) => item.id === next.id ? next : item);
      seen.set(next.id, next.state);
    } catch (error) {
      store.notice = message(error);
    }
  }

  onMount(() => restoreScroll(scroller));
</script>

{#if loaded && rows.length === 0}
  <div class="empty-state">
    <h2>No requests</h2>
  </div>
{:else}
  {#each rows as row (row.id)}
    <div class="book-row">
      <div class="book-link">
        <div class="post-column">
          <div class="post-meta">
            <strong class="truncate">{row.title}</strong>
            {#if all}
              <span class="meta truncate">@{row.requester}</span>
            {/if}
          </div>
          <p class="meta">{stateLabel(row)}</p>
        </div>
      </div>
      <div class="book-actions">
        {#if row.state === 'imported' && row.bookId}
          <button
            class="icon-button"
            aria-label="On your shelf"
            onclick={() => router.go(readerUrl(row.bookId!))}
          >
            <Icon name="check" size={18} />
          </button>
        {:else if row.state === 'failed' || row.state === 'canceled'}
          <button class="icon-button" aria-label="Retry" onclick={() => { void retry(row); }}>
            <Icon name="retry" size={18} />
          </button>
        {:else if row.state === 'requested' || row.state === 'downloading'}
          <button class="icon-button" aria-label="Cancel" onclick={() => { void cancel(row); }}>
            <Icon name="close" size={18} />
          </button>
        {/if}
      </div>
    </div>
  {/each}
{/if}

<style>
  .book-row {
    display: flex;
    align-items: flex-start;
    border-bottom: .6px solid var(--line);
    padding-right: 12px;
  }

  .book-link {
    flex: 1;
    min-width: 0;
    padding: 12px 4px 16px 16px;
  }

  .book-link p {
    margin-top: 3px;
    font-size: .9375rem;
    line-height: 1.33;
  }

  .book-row .post-meta {
    height: 21px;
  }

  .book-actions {
    display: flex;
    align-items: center;
    flex: 0 0 auto;
    margin-top: 12px;
    height: 21px;
  }

  .book-actions > .icon-button {
    width: 40px;
    height: 40px;
    padding: 0;
    line-height: 0;
    color: var(--mute);
  }
</style>
