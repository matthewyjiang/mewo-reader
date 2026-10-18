<script lang="ts">
  import Avatar from './Avatar.svelte';
  import Icon from './Icon.svelte';
  import Sheet from './Sheet.svelte';
  import { router } from '../lib/router.svelte';
  import { store } from '../lib/store.svelte';

  let { onclose, onprofile, onsignout }: {
    onclose: () => void;
    onprofile: (username: string) => void;
    onsignout: () => void;
  } = $props();
</script>

<Sheet label="Account" kind="drawer" {onclose}>
  {#if store.session}
    {@const username = store.session.username}
    <button class="drawer-profile" onclick={() => onprofile(username)}>
      <Avatar name={username} size={64} />
      <strong>{username}</strong>
      <span class="meta">@{username}</span>
    </button>
    <button class="drawer-row" onclick={() => onprofile(username)}>
      <Icon name="person" size={26} />
      Profile
    </button>
  {/if}
  <button class="drawer-row" onclick={() => { onclose(); router.go('/settings'); }}>
    <Icon name="settings" size={26} />
    Settings
  </button>
  <button class="drawer-row" onclick={onsignout}>
    <Icon name="logout" size={26} />
    Sign out
  </button>
</Sheet>

<style>
.drawer-profile {
  display: flex;
  width: 100%;
  align-items: flex-start;
  flex-direction: column;
  gap: 2px;
  padding: 16px 16px 12px;
  text-align: left;
  border-bottom: .6px solid var(--line);
}

.drawer-profile :global(.avatar) {
  margin-bottom: 10px;
}

.drawer-profile strong {
  font-size: 1.25rem;
  line-height: 1.2;
  overflow-wrap: anywhere;
}

.drawer-profile .meta {
  font-size: .9375rem;
  line-height: 1.2;
}

.drawer-row {
  display: flex;
  width: 100%;
  align-items: center;
  gap: 16px;
  min-height: 64px;
  padding: 0 16px;
  text-align: left;
  font-size: 1.375rem;
  line-height: 1.2;
  font-weight: 700;
}

.drawer-row:hover, .drawer-profile:hover {
  background: var(--hover);
}
</style>
