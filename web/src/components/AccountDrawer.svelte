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
  <div class="drawer-header">
    <button class="icon-button" aria-label="Close account drawer" onclick={onclose}>
      <Icon name="close" />
    </button>
  </div>
  {#if store.session}
    <button class="drawer-profile" onclick={() => onprofile(store.session!.username)}>
      <Avatar name={store.session.username} size={48} />
      <strong>{store.session.username}</strong>
      <span class="meta">@{store.session.username}</span>
    </button>
  {/if}
  <button
    class="drawer-row"
    onclick={() => { onclose(); router.go('/settings'); }}
  >
    <Icon name="settings" />
    Settings
  </button>
  <button class="drawer-row" onclick={onsignout}>Sign out</button>
</Sheet>
