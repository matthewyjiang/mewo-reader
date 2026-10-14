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
