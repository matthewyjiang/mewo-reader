<script lang="ts">
  import Sheet from './Sheet.svelte';
  import { message, store } from '../lib/store.svelte';

  let { onclose }: { onclose: () => void } = $props();
  let email = $state(store.me?.pendingEmail ?? store.me?.email ?? '');
  let current = $state('');
  let saving = $state(false);
  let error = $state('');
  let saved = $derived(Boolean(store.me?.email || store.me?.pendingEmail));

  async function change(next: string | null): Promise<void> {
    if (saving) return;
    if (!current) {
      error = 'Enter your current password.';
      return;
    }
    saving = true;
    error = '';
    try {
      const result = await store.api.setEmail(next, current);
      if (result === 'sent' && next !== null) store.notice = `Check ${next.trim()} for a link.`;
      await store.loadMe();
      onclose();
    } catch (err) {
      error = message(err);
    } finally {
      saving = false;
    }
  }

  function save(event: SubmitEvent): void {
    event.preventDefault();
    void change(email);
  }
</script>

<Sheet label="Email" onclose={() => { if (!saving) onclose(); }}>
  <form class="email-sheet" onsubmit={save}>
    <h2>Email</h2>
    <label class="field">
      <span>Email</span>
      <input
        class="text-field"
        type="email"
        autocomplete="email"
        autocapitalize="none"
        spellcheck="false"
        maxlength="254"
        required
        bind:value={email}
      />
    </label>
    <label class="field">
      <span>Current password</span>
      <input
        class="text-field"
        type="password"
        autocomplete="current-password"
        required
        bind:value={current}
      />
    </label>
    {#if error}<p class="error" role="alert">{error}</p>{/if}
    <div class="actions">
      {#if saved}
        <button
          type="button"
          class="text-button danger"
          disabled={saving}
          onclick={() => change(null)}
        >
          Remove email
        </button>
      {/if}
      <button type="submit" class="pill" disabled={saving}>{saving ? 'Saving…' : 'Save'}</button>
    </div>
  </form>
</Sheet>

<style>
.email-sheet {
  padding: 24px;
}

.field {
  display: flex;
  flex-direction: column;
  gap: 8px;
  margin-top: 16px;
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

.error {
  margin-top: 16px;
}

.actions {
  display: flex;
  gap: 16px;
  align-items: center;
  justify-content: flex-end;
  margin-top: 24px;
}

.actions .danger {
  margin-right: auto;
}
</style>
