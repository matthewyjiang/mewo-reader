<script lang="ts">
  import { ApiError } from '../lib/api';
  import type { BookRecord } from '../lib/models';
  import { message, store } from '../lib/store.svelte';
  import Sheet from './Sheet.svelte';

  let { book, onclose }: { book: BookRecord; onclose: () => void } = $props();

  let confirming = $state(false);
  let busy = $state(false);
  let error = $state('');
  let pending = $state<File | null>(null);
  let mismatch = $state(false);

  const canReport = $derived(
    book.reportable === true || (store.me?.isAdmin === true && book.hasRelease === true),
  );

  async function upload(file: File, force: boolean): Promise<void> {
    if (busy) return;
    busy = true;
    error = '';
    try {
      await store.api.replaceEpub(book.id, file, force);
      await store.load();
      onclose();
    } catch (err) {
      pending = file;
      mismatch = err instanceof ApiError && err.status === 409;
      error = message(err);
    } finally {
      busy = false;
    }
  }

  function picked(event: Event): void {
    const input = event.currentTarget;
    if (!(input instanceof HTMLInputElement)) return;
    const file = input.files?.[0];
    input.value = '';
    if (!file) return;
    mismatch = false;
    void upload(file, false);
  }

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
      {:else}
        <label class="pill">
          {busy ? 'Uploading…' : 'Upload replacement'}
          <input
            type="file"
            accept=".epub,application/epub+zip"
            disabled={busy}
            onchange={picked}
          />
        </label>
        {#if mismatch && pending}
          <button class="pill" disabled={busy} onclick={() => { void upload(pending!, true); }}>
            Replace anyway
          </button>
        {/if}
        {#if canReport}
          <button class="pill danger" disabled={busy} onclick={() => { confirming = true; }}>
            Wrong file
          </button>
        {/if}
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

.actions label {
  cursor: pointer;
  font: inherit;
}

.pill input {
  position: absolute;
  width: 1px;
  height: 1px;
  overflow: hidden;
  clip: rect(0 0 0 0);
}
</style>
