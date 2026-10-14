<script lang="ts">
  import { store, message } from '../lib/store.svelte';
  import Avatar from './Avatar.svelte';
  import Icon from './Icon.svelte';

  let mode = $state<'login' | 'register'>('login');
  let username = $state('');
  let password = $state('');
  let busy = $state(false);
  let error = $state('');
  let valid = $derived(/^[A-Za-z0-9_]{3,32}$/.test(username.trim()) && password.length >= 8);

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
    <h1>{mode === 'register' ? 'This is you.' : 'Your next paragraph awaits.'}</h1>
    <p class="muted">
      {mode === 'register'
        ? 'Your profile and public notes use this username.'
        : 'Sign in to your hosted shelf. Your place and likes come with you.'}
    </p>
    <div class="identity-preview">
      <Avatar name={username || 'Mewo'} />
      <div>
        <strong>{username || 'Your username'}</strong>
        <div class="meta">@{username || 'you'}</div>
      </div>
    </div>
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
      aria-describedby="username-help"
    />
    <p id="username-help" class="meta">3–32 characters: letters, digits, underscore.</p>
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
    />
    <p class="meta">At least 8 characters.</p>
    {#if error || store.notice}
      <p class="error" role="alert">{error || store.notice}</p>
    {/if}
    <button type="submit" class="pill auth-submit" disabled={!valid || busy}>
      {busy ? 'Connecting…' : mode === 'login' ? 'Sign in' : 'Create account'}
    </button>
    <button type="button" class="text-button" disabled={busy} onclick={switchMode}>
      {mode === 'login' ? 'Create an account' : 'Already have an account? Sign in'}
    </button>
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
  padding: 48px 28px 24px;
  display: flex;
  flex-direction: column;
  align-items: stretch;
  max-width: 460px;
  margin-inline: auto;
}

.auth-form h1 {
  font-size: 1.75rem;
  margin-bottom: 12px;
}

.auth-form > .identity-preview {
  margin-top: 32px;
  margin-bottom: 12px;
}

.auth-form > label {
  margin: 20px 0 8px;
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

.auth-form > .meta {
  margin-top: 6px;
}

.auth-form > .error {
  margin-top: 16px;
}

.auth-submit {
  margin-top: 28px;
  align-self: flex-end;
}

.auth-form > .text-button {
  margin-top: 12px;
  text-align: center;
}

@media (max-width: 760px) {
  .auth-page {
    border: 0;
  }
}
</style>
