<script lang="ts">
  import { onMount } from 'svelte';
  import AuthPage from '../components/AuthPage.svelte';
  import { ApiError } from '../lib/api';
  import { linkToken, withoutHash } from '../lib/emailLinks';
  import { router } from '../lib/router.svelte';
  import { message, store } from '../lib/store.svelte';

  // Read once, then drop the hash so the token does not stay in history.
  const token = linkToken(location.href);
  let phase = $state<'checking' | 'done' | 'expired' | 'failed'>(token ? 'checking' : 'expired');
  let email = $state('');
  let error = $state('');

  onMount(() => {
    if (location.hash) router.go(withoutHash(location.href), true);
    if (!token) return;
    void store.api.verifyEmail(token)
      .then((address) => {
        email = address;
        phase = 'done';
        if (store.session) void store.loadMe();
      })
      .catch((err: unknown) => {
        if (err instanceof ApiError && err.status === 400) {
          phase = 'expired';
          return;
        }
        error = message(err);
        phase = 'failed';
      });
  });
</script>

<AuthPage>
  {#if phase === 'checking'}
    <h1>Confirming…</h1>
  {:else if phase === 'done'}
    <h1>Email confirmed</h1>
    <p class="address">{email}</p>
    <button type="button" class="pill auth-submit" onclick={() => router.go('/', true)}>Open Mewo</button>
  {:else}
    <h1>{phase === 'expired' ? 'Link expired' : 'Email not confirmed'}</h1>
    {#if error}<p class="error" role="alert">{error}</p>{/if}
    <button type="button" class="pill auth-submit" onclick={() => router.go('/', true)}>Open Mewo</button>
  {/if}
</AuthPage>

<style>
.address {
  overflow-wrap: anywhere;
}
</style>
