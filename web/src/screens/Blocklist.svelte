<script lang="ts">
  import { onMount } from 'svelte';
  import { ApiError } from '../lib/api';
  import type { BlockedRelease } from '../lib/models';
  import { restoreScroll } from '../lib/screen';
  import { message, store } from '../lib/store.svelte';

  let { scroller }: { scroller: HTMLElement } = $props();

  let status = $state<'loading' | 'ready' | 'denied'>('loading');
  let items = $state<BlockedRelease[]>([]);
  let pending = $state('');
  let generation = 0;

  onMount(() => {
    restoreScroll(scroller);
    const ticket = ++generation;
    void load(ticket);
    return () => {
      generation += 1;
    };
  });

  async function load(ticket: number): Promise<void> {
    try {
      const next = await store.api.blocklist();
      if (ticket !== generation) return;
      items = next;
      status = 'ready';
    } catch (error) {
      if (ticket !== generation) return;
      status = 'denied';
      if (!(error instanceof ApiError && error.status === 403)) store.notice = message(error);
    }
  }

  async function unblock(row: BlockedRelease): Promise<void> {
    if (pending) return;
    pending = row.id;
    try {
      await store.api.unblock(row.id);
      items = items.filter((item) => item.id !== row.id);
    } catch (error) {
      store.notice = message(error);
    } finally {
      pending = '';
    }
  }
</script>

{#if status === 'loading'}
  <p class="empty-state">Loading…</p>
{:else if status === 'denied'}
  <p class="empty-state">Admins only.</p>
{:else if items.length === 0}
  <p class="empty-state">No blocked releases.</p>
{:else}
  <ul class="block-list">
    {#each items as row (row.id)}
      <li class="block-row">
        <span class="title">{row.bookTitle || row.title}</span>
        <button
          class="pill"
          disabled={pending === row.id}
          onclick={() => { void unblock(row); }}
        >
          {pending === row.id ? 'Unblocking…' : 'Unblock'}
        </button>
      </li>
    {/each}
  </ul>
{/if}

<style>
.block-list {
  list-style: none;
}

.block-row {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 16px;
  min-height: 64px;
  padding: 12px 16px;
  border-bottom: .6px solid var(--line);
}

.title {
  min-width: 0;
  font-size: 1.0625rem;
  line-height: 1.3;
}
</style>
