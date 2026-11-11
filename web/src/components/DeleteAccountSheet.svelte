<script lang="ts">
  import { onMount } from 'svelte';
  import Sheet from './Sheet.svelte';
  import { removalLines } from '../lib/accounts';
  import type { AccountRemoval } from '../lib/models';
  import { message, store } from '../lib/store.svelte';

  let { username, onclose, ondeleted }: {
    username: string;
    onclose: () => void;
    ondeleted: () => void;
  } = $props();

  let preview = $state<AccountRemoval | null>(null);
  let typed = $state('');
  let deleting = $state(false);
  let error = $state('');
  const ready = $derived(preview !== null && typed === username && !deleting);

  onMount(() => {
    let live = true;
    store.api.accountRemoval(username).then(
      (next) => { if (live) preview = next; },
      (err) => { if (live) error = message(err); },
    );
    return () => { live = false; };
  });

  async function remove(event: SubmitEvent): Promise<void> {
    event.preventDefault();
    if (!ready) return;
    deleting = true;
    error = '';
    try {
      await store.api.deleteAccount(username);
      ondeleted();
    } catch (err) {
      error = message(err);
    } finally {
      deleting = false;
    }
  }
</script>

<Sheet label="Delete account" onclose={() => { if (!deleting) onclose(); }}>
  <form class="delete-account" onsubmit={remove}>
    <h2>Delete {username}?</h2>
    {#if preview}
      <ul>
        {#each removalLines(preview) as line (line)}<li>{line}</li>{/each}
      </ul>
    {:else if !error}
      <p>Loading…</p>
    {/if}
    <label class="confirm-field">
      <span>Type {username} to confirm</span>
      <input
        class="text-field"
        autocomplete="off"
        autocapitalize="off"
        spellcheck="false"
        bind:value={typed}
      />
    </label>
    {#if error}<p class="error" role="alert">{error}</p>{/if}
    <div class="confirm-actions">
      <button type="button" class="text-button" disabled={deleting} onclick={onclose}>Keep</button>
      <button type="submit" class="pill danger-pill" disabled={!ready}>
        {deleting ? 'Deleting…' : 'Delete'}
      </button>
    </div>
  </form>
</Sheet>

<style>
.delete-account {
  padding: 24px;
}

ul {
  margin: 16px 0 0;
  padding-left: 20px;
  line-height: 1.41;
}

li + li {
  margin-top: 4px;
}

.delete-account > p {
  margin-top: 16px;
}

.confirm-field {
  display: flex;
  flex-direction: column;
  gap: 8px;
  margin-top: 24px;
  font-weight: 700;
}

.text-field {
  height: 40px;
  padding: 0 12px;
  color: var(--ink);
  background: var(--ground);
  border: 1px solid var(--line);
  border-radius: 4px;
  font: inherit;
  font-weight: 400;
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
