<script lang="ts">
  import { onMount } from 'svelte';
  import Avatar from '../components/Avatar.svelte';
  import DisplaySettings from '../components/DisplaySettings.svelte';
  import Icon from '../components/Icon.svelte';
  import { router } from '../lib/router.svelte';
  import { restoreScroll } from '../lib/screen';
  import { store } from '../lib/store.svelte';

  let { path, scroller, uploading, onadd, onsample, onprofile, onsignout }: {
    path: string;
    scroller: HTMLElement;
    uploading: boolean;
    onadd: () => void;
    onsample: () => void;
    onprofile: (username: string) => void;
    onsignout: () => void;
  } = $props();

  onMount(() => restoreScroll(scroller));
</script>

{#if path === '/settings'}
  <button class="settings-row" onclick={() => router.go('/settings/account')}>
    <span>
      <strong>Account</strong>
      <span class="meta">@{store.session?.username}</span>
    </span>
    <Icon name="chevron" />
  </button>
  <button class="settings-row" onclick={() => router.go('/settings/display')}>
    <span>
      <strong>Display</strong>
      <span class="meta">Twitter or X. Light or night. Font size. Mewo mode.</span>
    </span>
    <Icon name="chevron" />
  </button>
  <button class="settings-row" onclick={() => router.go('/settings/library')}>
    <span>
      <strong>Library</strong>
      <span class="meta">The shared hosted shelf.</span>
    </span>
    <Icon name="chevron" />
  </button>
{:else if path === '/settings/display'}
  <DisplaySettings />
{:else if path === '/settings/account' && store.session}
  <section class="settings-content">
    <div class="identity-preview">
      <Avatar name={store.session.username} />
      <div>
        <strong>{store.session.username}</strong>
        <p class="meta">@{store.session.username}</p>
      </div>
    </div>
    <p class="muted">
      Your hosted username is used on your profile and public notes.
      Account names are managed by the server.
    </p>
    <button class="text-button" onclick={() => onprofile(store.session!.username)}>
      View profile
    </button>
    <button class="text-button danger" onclick={onsignout}>Sign out</button>
  </section>
{:else if path === '/settings/library'}
  <section class="settings-content">
    <h2>Hosted library</h2>
    <p class="muted">
      One shared shelf for everyone on this server. Notes are public.
      Likes and reading place stay yours. Only the person who added a book can remove it.
    </p>
    <p class="meta">{location.origin}</p>
    <button class="pill" disabled={uploading} onclick={onadd}>Add an EPUB</button>
    <button class="text-button" disabled={uploading} onclick={onsample}>Open a sample</button>
  </section>
{:else}
  <div class="empty-state">
    <h2>That page isn't here</h2>
    <button class="pill" onclick={() => router.go('/settings')}>Back to Settings</button>
  </div>
{/if}

<style>
.settings-row {
  width: 100%;
  padding: 20px 16px;
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 16px;
  text-align: left;
  border-bottom: .6px solid var(--line);
}

.settings-row > span {
  display: flex;
  flex-direction: column;
  gap: 4px;
}

.settings-row strong {
  font-size: 1.0625rem;
}

.settings-row :global(svg) {
  color: var(--mute);
  flex-shrink: 0;
}

.settings-row:hover {
  background: var(--hover);
}

.settings-content {
  padding: 24px 16px;
  display: flex;
  flex-direction: column;
  align-items: flex-start;
  gap: 16px;
}


</style>
