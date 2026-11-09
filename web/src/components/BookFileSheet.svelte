<script lang="ts">
  import type { BookRecord } from '../lib/models';
  import { message, store } from '../lib/store.svelte';
  import Sheet from './Sheet.svelte';

  let { book, onclose }: { book: BookRecord; onclose: () => void } = $props();

  let confirming = $state(false);
  let busy = $state(false);
  let error = $state('');

  const canReport = $derived(
    book.reportable === true || (store.me?.isAdmin === true && book.hasRelease === true),
  );

  async function report(): Promise<void> {
    if (busy) return;
    busy = true;
    error = '';
    try {
      await store.api.reportWrongFile(book.id);
      await store.load();
      onclose();
    } catch (err) {
      error = message(err);
    } finally {
      busy = false;
    }
  }
</script>

<Sheet label="Book file" onclose={() => { if (!busy) onclose(); }}>
  <section class="book-file">
    <h2>{confirming ? 'Report wrong file?' : 'Book file'}</h2>
    {#if error}<p class="error" role="alert">{error}</p>{/if}
    <div class="actions">
      {#if confirming}
        <button class="pill" disabled={busy} onclick={() => { confirming = false; error = ''; }}>
          Keep
        </button>
        <button class="pill danger" disabled={busy} onclick={() => { void report(); }}>
          {busy ? 'Reporting…' : 'Report'}
        </button>
      {:else if canReport}
        <button class="pill danger" onclick={() => { confirming = true; }}>Wrong file</button>
      {/if}
    </div>
  </section>
</Sheet>

<style>
.book-file {
  padding: 24px;
}

.book-file h2 {
  font-size: 1.25rem;
  line-height: 1.3;
}

.actions {
  display: flex;
  flex-wrap: wrap;
  gap: 12px;
  margin-top: 24px;
}

.danger {
  background: var(--like);
}
</style>
