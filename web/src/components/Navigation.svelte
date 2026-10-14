<script lang="ts">
  import Avatar from './Avatar.svelte';
  import Brand from './Brand.svelte';
  import Icon from './Icon.svelte';
  import { chrome } from '../lib/chrome.svelte';
  import { router } from '../lib/router.svelte';
  import type { Tab } from '../lib/router.svelte';
  import { store } from '../lib/store.svelte';

  let { rail = false, onaccount }: { rail?: boolean; onaccount: () => void } = $props();
  const tabs: { id: Tab; label: string; path: string }[] = [
    { id: 'home', label: 'Home', path: '/' },
    { id: 'search', label: 'Search', path: '/search' },
    { id: 'likes', label: 'Likes', path: '/likes' },
  ];
</script>

<nav
  class={rail ? 'nav-rail' : 'tab-bar'}
  class:bar-hidden={!rail && !chrome.shown}
  aria-label="Primary navigation"
>
  {#if rail}
    <a
      class="rail-brand"
      href="/"
      aria-label="Mewo Home"
      onclick={(event) => { event.preventDefault(); router.go('/'); }}
    >
      <Brand />
    </a>
  {/if}
  {#each tabs as tab}
    <a
      href={tab.path}
      class:active={router.current.tab === tab.id}
      aria-label={tab.label}
      aria-current={router.url.pathname === tab.path ? 'page' : undefined}
      onclick={(event) => { event.preventDefault(); router.go(tab.path); }}
    >
      <Icon
        name={tab.id === 'likes' ? 'heart' : tab.id}
        filled={router.current.tab === tab.id && tab.id !== 'search'}
      />
      {#if rail}<span>{tab.label}</span>{/if}
    </a>
  {/each}
  {#if rail && store.session}
    <button onclick={onaccount} class="rail-account">
      <Avatar name={store.session.username} />
      <span>
        <strong>{store.session.username}</strong>
        <span class="meta">@{store.session.username}</span>
      </span>
      <Icon name="more" />
    </button>
  {/if}
</nav>
