<script lang="ts">
  import Sheet from './Sheet.svelte';
  import { visibleToOthers, type BookRecord } from '../lib/models';
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
    <h2>Remove {book.title}?</h2>
    <p class="muted">
      {#if visibleToOthers(book)}
        This removes it for everyone who can read it, plus likes, notes, and reading place.
      {:else}
        This removes it from your library, plus likes, notes, and reading place.
        It is private, so nobody else can read it.
      {/if}
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

<style>
.confirm-remove {
  padding: 24px;
}

.confirm-remove p {
  margin-top: 16px;
}

.confirm-actions {
  display: flex;
  gap: 16px;
  justify-content: flex-end;
  margin-top: 24px;
}

.danger-pill {
  background: var(--like);
}


</style>
