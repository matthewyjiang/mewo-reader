<script lang="ts">
  import Sheet from './Sheet.svelte';
  import { message, store } from '../lib/store.svelte';

  let { onclose }: { onclose: () => void } = $props();
  let current = $state('');
  let next = $state('');
  let saving = $state(false);
  let error = $state('');

  async function save(event: SubmitEvent): Promise<void> {
    event.preventDefault();
    if (saving) return;
    if (next.length < 8) {
      error = 'Password must be at least 8 characters.';
      return;
    }
    saving = true;
    error = '';
    try {
      await store.api.changePassword(current, next);
      store.notice = 'Password changed. Other devices are signed out.';
      onclose();
    } catch (err) {
      error = message(err);
    } finally {
      saving = false;
    }
  }
</script>

<Sheet label="Change password" onclose={() => { if (!saving) onclose(); }}>
  <form class="change-password" onsubmit={save}>
    <h2>Change password</h2>
    <label class="field">
      <span>Current</span>
      <input class="text-field" type="password" autocomplete="current-password" required bind:value={current} />
    </label>
    <label class="field">
      <span>New</span>
      <input
        class="text-field"
        type="password"
        autocomplete="new-password"
        required
        bind:value={next}
      />
    </label>
    {#if error}<p class="error" role="alert">{error}</p>{/if}
    <div class="actions">
      <button type="button" class="text-button" disabled={saving} onclick={onclose}>Cancel</button>
      <button type="submit" class="pill" disabled={saving}>{saving ? 'Saving…' : 'Save'}</button>
    </div>
  </form>
</Sheet>

<style>
.change-password {
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
  justify-content: flex-end;
  margin-top: 24px;
}
</style>
