<script lang="ts">
  import { checkHeading, type CheckStatus } from '../lib/invites';
  import type { InstanceInfo } from '../lib/models';
  import { router } from '../lib/router.svelte';
  import { store, message } from '../lib/store.svelte';
  import Icon from './Icon.svelte';

  let { invite = null }: { invite: string | null } = $props();

  const usernameRule = '3–32 characters: letters, digits, underscore.';
  const passwordRule = 'At least 8 characters.';

  let mode = $state<'login' | 'register' | 'reset'>('login');
  let username = $state('');
  let password = $state('');
  let email = $state('');
  let login = $state('');
  let sent = $state(false);
  let busy = $state(false);
  let error = $state('');
  let ignoreInvite = $state(false);
  let check = $state<'idle' | 'checking' | 'failed' | CheckStatus>('idle');
  let usernameValue = $derived(username.trim());
  let usernameOk = $derived(/^[A-Za-z0-9_]{3,32}$/.test(usernameValue));
  let passwordOk = $derived(password.length >= 8);
  let valid = $derived(usernameOk && passwordOk);
  let usernameError = $derived(usernameValue && !usernameOk ? usernameRule : '');
  let passwordError = $derived(password && !passwordOk ? passwordRule : '');
  let activeInvite = $derived(ignoreInvite ? null : invite);
  let checking = $derived(activeInvite !== null && check === 'checking');
  let dead = $derived(
    activeInvite && (check === 'used' || check === 'expired' || check === 'invalid') ? check : null,
  );
  // null while the instance call is in flight or failed: keep today's link.
  let instance = $state<InstanceInfo | null>(null);
  let allowSignup = $derived(instance?.allowSignup ?? null);
  let passwordReset = $derived(instance?.passwordReset === true);

  $effect(() => {
    const controller = new AbortController();
    instance = null;
    void fetch('/v1/instance', { signal: controller.signal })
      .then(async (response) => {
        if (!response.ok) throw new Error(String(response.status));
        const body: unknown = await response.json();
        if (!body || typeof body !== 'object' || !('allowSignup' in body)
          || typeof body.allowSignup !== 'boolean') {
          throw new Error('instance');
        }
        // Older servers have no passwordReset.
        const reset = 'passwordReset' in body && body.passwordReset === true;
        return { allowSignup: body.allowSignup, passwordReset: reset };
      })
      .then((info) => {
        if (!controller.signal.aborted) instance = info;
      })
      .catch(() => {
        if (!controller.signal.aborted) instance = null;
      });
    return () => controller.abort();
  });

  $effect(() => {
    if (activeInvite) return;
    if (allowSignup === false && mode === 'register') mode = 'login';
  });

  $effect(() => {
    const token = invite;
    ignoreInvite = false;
    if (!token) {
      check = 'idle';
      return;
    }
    const controller = new AbortController();
    check = 'checking';
    mode = 'register';
    void store.api.checkInvite(token, controller.signal)
      .then((status) => {
        if (!controller.signal.aborted) check = status;
      })
      .catch((err: unknown) => {
        if (controller.signal.aborted) return;
        if (err instanceof DOMException && err.name === 'AbortError') return;
        check = 'failed';
      });
    return () => controller.abort();
  });

  async function submit(event: SubmitEvent): Promise<void> {
    event.preventDefault();
    if (!valid || busy || checking) return;
    busy = true;
    error = '';
    if (mode === 'reset') return;
    const token = mode === 'register' ? activeInvite ?? undefined : undefined;
    const address = mode === 'register' && passwordReset ? email : undefined;
    try {
      await store.signIn(mode, username, password, token, address);
      if (token) router.go('/', true);
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

  function forgot(): void {
    mode = 'reset';
    login = username;
    sent = false;
    error = '';
    store.notice = '';
  }

  function backToLogin(): void {
    mode = 'login';
    error = '';
    store.notice = '';
  }

  async function sendLink(event: SubmitEvent): Promise<void> {
    event.preventDefault();
    if (!login.trim() || busy) return;
    busy = true;
    error = '';
    try {
      await store.api.requestReset(login);
      sent = true;
    } catch (err) {
      error = message(err);
    } finally {
      busy = false;
    }
  }

  function signInInstead(): void {
    ignoreInvite = true;
    mode = 'login';
    error = '';
    store.notice = '';
  }

  // Clearing the query leaves this component mounted, so registration mode has to
  // be dropped here. The signup setting only forces login when signups are closed.
  function dismissInvite(): void {
    mode = 'login';
    error = '';
    store.notice = '';
    router.go('/', true);
  }
</script>

<div class="auth-page">
  <header class="auth-bar">
    <Icon name="book" size={28} />
    <span class="sr-only">Mewo</span>
  </header>
  {#if dead}
    <div class="auth-form">
      <h1>{checkHeading(dead)}</h1>
      <button type="button" class="pill auth-submit" onclick={dismissInvite}>
        Sign in
      </button>
    </div>
  {:else if mode === 'reset'}
  <form class="auth-form" onsubmit={sendLink}>
    <h1>Reset password</h1>
    <label for="login">Username or email</label>
    <input
      id="login"
      name="login"
      autocomplete="username"
      autocapitalize="none"
      spellcheck="false"
      maxlength="320"
      required
      bind:value={login}
      oninput={() => sent = false}
      disabled={busy}
    />
    {#if error}
      <p class="error" role="alert">{error}</p>
    {:else if sent}
      <p class="sent" role="status">If that account has an email, we sent a link.</p>
    {/if}
    <button type="submit" class="pill auth-submit" disabled={!login.trim() || busy}>
      {busy ? 'Sending…' : 'Send link'}
    </button>
    <button type="button" class="text-button" disabled={busy} onclick={backToLogin}>
      Sign in instead
    </button>
  </form>
  {:else}
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
      disabled={busy || checking}
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
      disabled={busy || checking}
      aria-invalid={passwordError ? 'true' : undefined}
      aria-describedby={passwordError ? 'password-error' : undefined}
    />
    {#if passwordError}
      <p id="password-error" class="error">{passwordError}</p>
    {/if}
    {#if mode === 'register' && passwordReset}
      <label for="email">Email (optional)</label>
      <input
        id="email"
        name="email"
        type="email"
        autocomplete="email"
        autocapitalize="none"
        spellcheck="false"
        maxlength="254"
        bind:value={email}
        disabled={busy || checking}
      />
    {/if}
    {#if error || store.notice}
      <p class="error" role="alert">{error || store.notice}</p>
    {/if}
    <button type="submit" class="pill auth-submit" disabled={!valid || busy || checking}>
      {busy ? 'Connecting…' : mode === 'login' ? 'Sign in' : 'Create account'}
    </button>
    {#if mode === 'login' && passwordReset}
      <button type="button" class="text-button" disabled={busy} onclick={forgot}>
        Forgot password?
      </button>
    {/if}
    {#if mode === 'register'}
      <button
        type="button"
        class="text-button"
        disabled={busy}
        onclick={activeInvite ? signInInstead : switchMode}
      >
        Sign in instead
      </button>
    {:else if allowSignup !== false}
      <button type="button" class="text-button" disabled={busy} onclick={switchMode}>
        Create an account
      </button>
    {/if}
  </form>
  {/if}
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

.auth-form > .error,
.auth-form > .sent {
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
