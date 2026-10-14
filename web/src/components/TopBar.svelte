<script lang="ts">
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
        <input type="search" placeholder="Search Mewo" bind:value={query} />
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
