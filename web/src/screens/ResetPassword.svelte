<script lang="ts">
  import { onMount } from 'svelte';
  import AuthPage from '../components/AuthPage.svelte';
  import { ApiError } from '../lib/api';
  import { linkToken, withoutHash } from '../lib/emailLinks';
  import { router } from '../lib/router.svelte';
  import { message, store } from '../lib/store.svelte';

  // Read once, then drop the hash so the token does not stay in history.
  const token = linkToken(location.href);
  let phase = $state<'form' | 'done' | 'expired'>(token ? 'form' : 'expired');
  let password = $state('');
  let busy = $state(false);
  let error = $state('');
  let passwordOk = $derived(password.length >= 8);
  let passwordError = $derived(password && !passwordOk ? 'At least 8 characters.' : '');

  onMount(() => {
    if (location.hash) router.go(withoutHash(location.href), true);
  });

  async function submit(event: SubmitEvent): Promise<void> {
    event.preventDefault();
    if (!token || !passwordOk || busy) return;
    busy = true;
    error = '';
    try {
      await store.api.redeemReset(token, password);
      // Every session for that account just ended, this one included.
      if (store.session) store.clear();
      phase = 'done';
    } catch (err) {
      if (err instanceof ApiError && err.status === 400 && err.message === 'This link has expired.') {
        phase = 'expired';
      } else {
        error = message(err);
      }
    } finally {
      busy = false;
    }
  }
</script>

<AuthPage>
  {#if phase === 'done'}
    <h1>Password changed</h1>
    <button type="button" class="pill auth-submit" onclick={() => router.go('/', true)}>Sign in</button>
  {:else if phase === 'expired'}
    <h1>Link expired</h1>
    <button type="button" class="pill auth-submit" onclick={() => router.go('/', true)}>Sign in</button>
  {:else}
    <form onsubmit={submit}>
      <h1>Reset password</h1>
      <label for="new-password">New password</label>
      <input
        id="new-password"
        name="new-password"
        type="password"
        autocomplete="new-password"
        minlength="8"
        required
        bind:value={password}
        disabled={busy}
        aria-invalid={passwordError ? 'true' : undefined}
        aria-describedby={passwordError ? 'new-password-error' : undefined}
      />
      {#if passwordError}
        <p id="new-password-error" class="error">{passwordError}</p>
      {/if}
      {#if error}
        <p class="error" role="alert">{error}</p>
      {/if}
      <button type="submit" class="pill auth-submit" disabled={!passwordOk || busy}>
        {busy ? 'Saving…' : 'Set password'}
      </button>
    </form>
  {/if}
</AuthPage>
