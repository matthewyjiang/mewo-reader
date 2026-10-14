<script lang="ts">
  import Sheet from './Sheet.svelte';
  import type { BookRecord } from '../lib/models';
  import { store, message } from '../lib/store.svelte';

  let { book, onclose }: { book: BookRecord; onclose: () => void } = $props();
  let removing = $state(false);
  let error = $state('');

  async function remove(): Promise<void> {
    if (removing) return;
    removing = true;
    error = '';
    try {
      await store.remove(book);
      onclose();
    } catch (err) {
      error = message(err);
    } finally {
      removing = false;
    }
  }
</script>

<Sheet label="Remove book" onclose={() => { if (!removing) onclose(); }}>
  <section class="confirm-remove">
    <h2>Remove from the shared library?</h2>
    <p class="muted">
      This removes {book.title} for everyone on this server, plus likes, notes,
      and reading place.
    </p>
    {#if error}<p class="error" role="alert">{error}</p>{/if}
    <div class="confirm-actions">
      <button class="text-button" disabled={removing} onclick={onclose}>Keep</button>
      <button class="pill danger-pill" disabled={removing} onclick={remove}>
        {removing ? 'Removing…' : 'Remove'}
      </button>
    </div>
  </section>
</Sheet>
