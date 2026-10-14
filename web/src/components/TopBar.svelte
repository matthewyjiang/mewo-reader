<script lang="ts">
  import { tick } from 'svelte';
  import Avatar from './Avatar.svelte';
  import Brand from './Brand.svelte';
  import Icon from './Icon.svelte';
  import { chrome } from '../lib/chrome.svelte';
  import { router } from '../lib/router.svelte';
  import { store } from '../lib/store.svelte';

  let { title, root, reader, profile, busy, count, query = $bindable(), onaccount, onchapters }: {
    title: string;
    root: boolean;
    reader: boolean;
    profile: boolean;
    busy: boolean;
    count: number;
    query: string;
    onaccount: () => void;
    onchapters: () => void;
  } = $props();
  let searchInput = $state<HTMLInputElement>();
  $effect(() => {
    const key = router.current.key;
    if (router.fromTab && router.url.pathname === '/search') {
      void tick().then(() => {
        if (router.current.key === key) searchInput?.focus({ preventScroll: true });
      });
    }
  });
</script>

<header
  class="top-bar"
  class:bar-hidden={!chrome.shown && !busy && router.url.pathname !== '/search'}
>
  {#if root}
    <button
      class="icon-button profile-button"
      aria-label="Open account drawer"
      onclick={onaccount}
    >
      <Avatar name={store.session?.username ?? 'M'} size={32} />
    </button>
    {#if router.url.pathname === '/search'}
      <label class="search-field">
        <Icon name="search" size={18} />
        <span class="sr-only">Search books and opened posts</span>
        <input bind:this={searchInput} type="search" placeholder="Search Mewo" bind:value={query} />
        {#if query}
          <button
            class="icon-button"
            aria-label="Clear search"
            onclick={() => query = ''}
          >
            <Icon name="close" size={16} />
          </button>
        {/if}
      </label>
    {:else}
      <div class="center-mark"><Brand /></div>
    {/if}
  {:else}
    <button class="icon-button" aria-label="Back" onclick={() => router.back()}>
      <Icon name="back" />
    </button>
    {#if reader}
      <button
        class="bar-title"
        aria-label={`Chapters, ${title}`}
        disabled={busy}
        onclick={onchapters}
      >
        {title}
      </button>
    {:else if profile}
      <div class="profile-title">
        <strong>{title}</strong>
        <span class="meta">{count} {count === 1 ? 'post' : 'posts'}</span>
      </div>
    {:else}
      <h1 class="bar-title">{title}</h1>
    {/if}
  {/if}
</header>

<style>
.top-bar {
  height: 56px;
  position: absolute;
  z-index: 5;
  inset: 0 0 auto;
  display: flex;
  align-items: center;
  gap: 4px;
  padding: 0 8px;
  border-bottom: .6px solid var(--line);
  background: var(--ground);
  transition: transform 200ms ease-out;
}

.top-bar.bar-hidden {
  transform: translateY(-100%);
  pointer-events: none;
}

.center-mark {
  position: absolute;
  left: 50%;
  transform: translateX(-50%);
  display: flex;
  align-items: center;
  pointer-events: none;
}


.bar-title {
  font-size: 1.0625rem;
  font-weight: 700;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
  flex: 1;
  text-align: center;
  padding-right: 44px;
  min-width: 0;
}

button.bar-title {
  height: 100%;
}

.profile-title {
  display: flex;
  flex-direction: column;
  gap: 2px;
  min-width: 0;
  padding-left: 8px;
}


.search-field {
  display: flex;
  align-items: center;
  gap: 8px;
  flex: 1;
  background: var(--hover);
  border: 1px solid var(--line);
  padding: 0 12px;
  height: 38px;
  margin: 0 8px;
  border-radius: 999px;
  color: var(--mute);
  min-width: 0;
}

.search-field input {
  border: 0;
  outline: 0;
  min-width: 0;
  flex: 1;
  padding: 0;
  background: transparent;
  color: var(--ink);
  font-size: .9375rem;
}

.search-field:focus-within {
  border-color: var(--accent);
}

.search-field input:focus-visible {
  outline: none;
}

.search-field .icon-button {
  min-height: 32px;
  min-width: 28px;
}

input[type='search']::-webkit-search-cancel-button {
  display: none;
}

.top-bar:focus-within {
  transform: none;
  pointer-events: auto;
}
</style>
