<script lang="ts">
  import Avatar from './Avatar.svelte';
  import Brand from './Brand.svelte';
  import Icon from './Icon.svelte';
  import { chrome } from '../lib/chrome.svelte';
  import { profileUrl, router } from '../lib/router.svelte';
  import type { Tab } from '../lib/router.svelte';
  import { store } from '../lib/store.svelte';

  let { rail = false, onaccount }: { rail?: boolean; onaccount: () => void } = $props();
  const tabs: { id: Tab; label: string; path: string }[] = [
    { id: 'home', label: 'Home', path: '/' },
    { id: 'search', label: 'Search', path: '/search' },
    { id: 'likes', label: 'Likes', path: '/likes' },
  ];
  let ownProfile = $derived.by(() => {
    const name = store.session?.username;
    if (!name || !router.url.pathname.startsWith('/profiles/')) return false;
    let handle = router.url.pathname.slice('/profiles/'.length);
    try { handle = decodeURIComponent(handle); } catch { /* keep the raw segment */ }
    return handle.toLowerCase() === name.toLowerCase();
  });
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
      onclick={(event) => { event.preventDefault(); router.selectTab('/'); }}
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
      onclick={(event) => { event.preventDefault(); router.selectTab(tab.path); }}
    >
      <Icon
        name={tab.id === 'likes' ? 'heart' : tab.id}
        filled={router.current.tab === tab.id && tab.id !== 'search'}
      />
      {#if rail}<span>{tab.label}</span>{/if}
    </a>
  {/each}
  {#if rail && store.session}
    <a
      href={profileUrl(store.session.username)}
      class:active={ownProfile}
      aria-label="Profile"
      aria-current={ownProfile ? 'page' : undefined}
      onclick={(event) => {
        event.preventDefault();
        router.go(profileUrl(store.session!.username));
      }}
    >
      <Icon name="person" filled={ownProfile} />
      <span>Profile</span>
    </a>
  {/if}
  {#if rail && store.session}
    <button onclick={onaccount} class="rail-account">
      <Avatar name={store.session.username} />
      <strong>{store.session.username}</strong>
      <Icon name="more" />
    </button>
  {/if}
</nav>

<style>
.tab-bar {
  position: absolute;
  z-index: 5;
  inset: auto 0 0;
  height: 52px;
  border-top: .6px solid var(--line);
  background: var(--ground);
  display: flex;
  align-items: stretch;
  justify-content: space-around;
  transition: transform 200ms ease-out;
}

.tab-bar a {
  flex: 1;
  display: flex;
  align-items: center;
  justify-content: center;
  color: var(--mute);
}

.tab-bar a.active {
  color: var(--ink);
}

.tab-bar a:hover {
  background: var(--hover);
}

.tab-bar.bar-hidden {
  transform: translateY(100%);
  pointer-events: none;
}


.nav-rail {
  grid-column: 1;
  grid-row: 1;
  justify-self: end;
  width: 230px;
  padding: 4px 16px;
  display: flex;
  flex-direction: column;
  align-items: flex-start;
  gap: 12px;
  min-height: 0;
}

.rail-brand {
  padding: 4px 12px;
}

.nav-rail > a:not(.rail-brand) {
  display: flex;
  gap: 20px;
  align-items: center;
  padding: 12px;
  font-size: 1.25rem;
  border-radius: 999px;
}

.nav-rail > a:hover, .rail-account:hover {
  background: var(--hover);
}

.nav-rail > a.active {
  font-weight: 700;
}

.rail-account {
  margin-top: auto;
  margin-bottom: 16px;
  display: flex;
  align-items: center;
  gap: 12px;
  border-radius: 999px;
  width: 100%;
  padding: 8px;
  text-align: left;
}

.rail-account > strong {
  flex: 1;
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  font-size: 1.0625rem;
}

.rail-account :global(.avatar) {
  flex: 0 0 40px;
}


.tab-bar:focus-within {
  transform: none;
  pointer-events: auto;
}

@media (min-width: 1000px) {
  .tab-bar {
    display: none;
  }
}

@media (min-width: 761px) and (max-width: 999px) {
  .nav-rail {
    width: 72px;
    padding-inline: 8px;
    align-items: center;
  }
  .nav-rail > a:not(.rail-brand) {
    padding: 12px;
  }
  .nav-rail > a:not(.rail-brand) > span,
  .rail-account > strong, .rail-account > :global(svg) {
    display: none;
  }
  .rail-account {
    width: auto;
  }
}

@media (max-width: 760px) {
  .nav-rail {
    display: none;
  }
}
</style>
