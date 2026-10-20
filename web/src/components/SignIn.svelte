<script lang="ts">
  import { store, message } from '../lib/store.svelte';
  import Icon from './Icon.svelte';

  const usernameRule = '3–32 characters: letters, digits, underscore.';
  const passwordRule = 'At least 8 characters.';

  let mode = $state<'login' | 'register'>('login');
  let username = $state('');
  let password = $state('');
  let busy = $state(false);
  let error = $state('');
  let usernameValue = $derived(username.trim());
  let usernameOk = $derived(/^[A-Za-z0-9_]{3,32}$/.test(usernameValue));
  let passwordOk = $derived(password.length >= 8);
  let valid = $derived(usernameOk && passwordOk);
  let usernameError = $derived(usernameValue && !usernameOk ? usernameRule : '');
  let passwordError = $derived(password && !passwordOk ? passwordRule : '');
  // null while the instance call is in flight or failed: keep today's link.
  let allowSignup = $state<boolean | null>(null);

  $effect(() => {
    const controller = new AbortController();
    allowSignup = null;
    void fetch('/v1/instance', { signal: controller.signal })
      .then(async (response) => {
        if (!response.ok) throw new Error(String(response.status));
        const body: unknown = await response.json();
        if (!body || typeof body !== 'object' || !('allowSignup' in body)
          || typeof body.allowSignup !== 'boolean') {
          throw new Error('instance');
        }
        return body.allowSignup;
      })
      .then((open) => {
        if (!controller.signal.aborted) allowSignup = open;
      })
      .catch(() => {
        if (!controller.signal.aborted) allowSignup = null;
      });
    return () => controller.abort();
  });

  $effect(() => {
    if (allowSignup === false && mode === 'register') mode = 'login';
  });

  async function submit(event: SubmitEvent): Promise<void> {
    event.preventDefault();
    if (!valid || busy) return;
    busy = true;
    error = '';
    try {
      await store.signIn(mode, username, password);
    } catch (err) {
      error = message(err);
    } finally {
      busy = false;
    }
  }

  function switchMode(): void {
    mode = mode === 'login' ? 'register' : 'login';
    error = '';
    store.notice = '';
  }
</script>

<div class="auth-page">
  <header class="auth-bar">
    <Icon name="book" size={28} />
    <span class="sr-only">Mewo</span>
  </header>
  <form class="auth-form" onsubmit={submit}>
    <h1>{mode === 'register' ? 'Create account' : 'Sign in'}</h1>
    <label for="username">Username</label>
    <input
      id="username"
      name="username"
      autocomplete="username"
      autocapitalize="none"
      spellcheck="false"
      pattern={'[A-Za-z0-9_]{3,32}'}
      minlength="3"
      maxlength="32"
      required
      bind:value={username}
      disabled={busy}
      aria-invalid={usernameError ? 'true' : undefined}
      aria-describedby={usernameError ? 'username-error' : undefined}
    />
    {#if usernameError}
      <p id="username-error" class="error">{usernameError}</p>
    {/if}
    <label for="password">Password</label>
    <input
      id="password"
      name="password"
      type="password"
      autocomplete={mode === 'register' ? 'new-password' : 'current-password'}
      minlength="8"
      required
      bind:value={password}
      disabled={busy}
      aria-invalid={passwordError ? 'true' : undefined}
      aria-describedby={passwordError ? 'password-error' : undefined}
    />
    {#if passwordError}
      <p id="password-error" class="error">{passwordError}</p>
    {/if}
    {#if error || store.notice}
      <p class="error" role="alert">{error || store.notice}</p>
    {/if}
    <button type="submit" class="pill auth-submit" disabled={!valid || busy}>
      {busy ? 'Connecting…' : mode === 'login' ? 'Sign in' : 'Create account'}
    </button>
    {#if mode === 'register'}
      <button type="button" class="text-button" disabled={busy} onclick={switchMode}>
        Sign in instead
      </button>
    {:else if allowSignup !== false}
      <button type="button" class="text-button" disabled={busy} onclick={switchMode}>
        Create an account
      </button>
    {/if}
  </form>
</div>

<style>
.auth-page {
  max-width: 600px;
  margin-inline: auto;
  min-height: 100dvh;
  padding-top: env(safe-area-inset-top);
  padding-bottom: env(safe-area-inset-bottom);
  border-inline: 1px solid var(--line);
}

.auth-bar {
  height: 56px;
  display: flex;
  align-items: center;
  justify-content: center;
  border-bottom: .6px solid var(--line);
}

.auth-form {
  padding: 24px 16px;
  display: flex;
  flex-direction: column;
  align-items: stretch;
  max-width: 460px;
  margin-inline: auto;
}

.auth-form h1 {
  font-size: 1.375rem;
  line-height: 52px;
  margin-bottom: 8px;
}

.auth-form > label {
  margin: 8px 0;
  min-height: 24px;
  font-weight: 700;
}

.auth-form > input {
  color: var(--ink);
  background: var(--ground);
  border: 1px solid var(--line);
  border-radius: 4px;
  padding: 12px;
  min-height: 48px;
  width: 100%;
}

.auth-form > input:focus {
  border-color: var(--accent);
}

.auth-form > .error {
  margin-top: 6px;
}

.auth-submit {
  margin-top: 20px;
  width: 100%;
}

.auth-form > .text-button {
  margin-top: 4px;
  text-align: center;
}

@media (max-width: 760px) {
  .auth-page {
    border: 0;
  }
}
</style>
