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
    </button>
  {/if}
  <button
    class="drawer-row"
    onclick={() => { onclose(); router.go('/settings'); }}
  >
    <Icon name="settings" />
    Settings
  </button>
  <button class="drawer-row" onclick={onsignout}>
    <span class="drawer-slot" aria-hidden="true"></span>
    Sign out
  </button>
</Sheet>

<style>
.drawer-header {
  display: flex;
  justify-content: flex-end;
  padding: 8px;
}

.drawer-profile {
  display: flex;
  width: 100%;
  align-items: flex-start;
  flex-direction: column;
  gap: 4px;
  padding: 8px 16px 16px;
  text-align: left;
  border-bottom: .6px solid var(--line);
}

.drawer-profile :global(.avatar) {
  margin-bottom: 8px;
}

.drawer-profile strong {
  font-size: 1.25rem;
  line-height: 1.2;
  overflow-wrap: anywhere;
}

.drawer-row {
  display: flex;
  width: 100%;
  align-items: center;
  gap: 16px;
  min-height: 52px;
  padding: 0 16px;
  text-align: left;
  font-size: 1.0625rem;
  font-weight: 700;
}

.drawer-slot {
  width: 22px;
  height: 22px;
  flex: 0 0 22px;
}

.drawer-row:hover {
  background: var(--hover);
}


</style>
