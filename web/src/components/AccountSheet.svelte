<script lang="ts">
  import Sheet from './Sheet.svelte';
  import type { AdminUser } from '../lib/models';
  import { copyText } from '../lib/share';
  import { message, store } from '../lib/store.svelte';

  let { user, onclose, onchange, ondelete }: {
    user: AdminUser;
    onclose: () => void;
    onchange: (user: AdminUser) => void;
    ondelete: () => void;
  } = $props();

  type Step = 'menu' | 'reset' | 'disable' | 'password';
  let step = $state<Step>('menu');
  let busy = $state(false);
  let error = $state('');
  // Shown once. Never stored outside this component.
  let password = $state('');
  let copied = $state(false);

  function go(next: Step): void {
    step = next;
    error = '';
  }

  async function run(action: () => Promise<void>): Promise<void> {
    if (busy) return;
    busy = true;
    error = '';
    try {
      await action();
    } catch (err) {
      error = message(err);
    } finally {
      busy = false;
    }
  }

  const reset = () => run(async () => {
    password = await store.api.resetPassword(user.username);
    step = 'password';
  });

  const setDisabled = (disabled: boolean) => run(async () => {
    onchange(await store.api.setDisabled(user.username, disabled));
    onclose();
  });

  async function copy(): Promise<void> {
    try {
      await copyText(password, 'Select the password and copy it manually.');
      copied = true;
    } catch (err) {
      error = message(err);
    }
  }
</script>

<Sheet label={`Manage ${user.username}`} onclose={() => { if (!busy) onclose(); }}>
  <section class="account-sheet">
    {#if step === 'menu'}
      <h2>{user.username}</h2>
      <div class="menu">
        <button class="pill" disabled={busy} onclick={() => go('reset')}>Reset password</button>
        {#if user.disabled}
          <button class="pill" disabled={busy} onclick={() => setDisabled(false)}>
            {busy ? 'Enabling…' : 'Enable'}
          </button>
        {:else}
          <button class="pill" disabled={busy} onclick={() => go('disable')}>Disable</button>
        {/if}
        <button class="pill danger-pill" disabled={busy} onclick={ondelete}>Delete account</button>
      </div>
      {#if error}<p class="error" role="alert">{error}</p>{/if}
      <div class="actions">
        <button class="text-button" disabled={busy} onclick={onclose}>Close</button>
      </div>
    {:else if step === 'reset'}
      <h2>Reset {user.username}'s password?</h2>
      <p>Signs {user.username} out everywhere.</p>
      {#if error}<p class="error" role="alert">{error}</p>{/if}
      <div class="actions">
        <button class="text-button" disabled={busy} onclick={() => go('menu')}>Cancel</button>
        <button class="pill" disabled={busy} onclick={reset}>{busy ? 'Resetting…' : 'Reset'}</button>
      </div>
    {:else if step === 'disable'}
      <h2>Disable {user.username}?</h2>
      <p>Signs {user.username} out everywhere. Books and posts stay.</p>
      {#if error}<p class="error" role="alert">{error}</p>{/if}
      <div class="actions">
        <button class="text-button" disabled={busy} onclick={() => go('menu')}>Cancel</button>
        <button class="pill danger-pill" disabled={busy} onclick={() => setDisabled(true)}>
          {busy ? 'Disabling…' : 'Disable'}
        </button>
      </div>
    {:else}
      <h2>New password for {user.username}</h2>
      <div class="password-row">
        <label class="password-field">
          <span class="sr-only">Temporary password</span>
          <input class="text-field" readonly value={password} onfocus={(event) => event.currentTarget.select()} />
        </label>
        <button class="pill" onclick={() => void copy()}>{copied ? 'Copied' : 'Copy'}</button>
      </div>
      {#if error}<p class="error" role="alert">{error}</p>{/if}
      <div class="actions">
        <button class="pill" onclick={onclose}>Done</button>
      </div>
    {/if}
  </section>
</Sheet>

<style>
.account-sheet {
  padding: 24px;
}

.account-sheet p {
  margin-top: 16px;
}

.menu {
  display: flex;
  flex-direction: column;
  gap: 12px;
  margin-top: 24px;
}

.actions {
  display: flex;
  gap: 16px;
  justify-content: flex-end;
  margin-top: 24px;
}

.danger-pill {
  background: var(--like);
}

.danger-pill:hover:not(:disabled) {
  background: color-mix(in srgb, var(--like) 85%, var(--ink));
}

.password-row {
  display: flex;
  align-items: center;
  gap: 12px;
  margin-top: 16px;
}

.password-field {
  flex: 1;
  min-width: 0;
}

.text-field {
  width: 100%;
  height: 40px;
  padding: 0 12px;
  color: var(--ink);
  background: var(--ground);
  border: 1px solid var(--line);
  border-radius: 4px;
  font: inherit;
  font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
}
</style>
