<script lang="ts">
  import { onMount, tick, untrack } from 'svelte';
  import Avatar from '../components/Avatar.svelte';
  import BookRow from '../components/BookRow.svelte';
  import Post from '../components/Post.svelte';
  import ThreadRow from '../components/ThreadRow.svelte';
  import UnderlineTabs from '../components/UnderlineTabs.svelte';
  import { ApiError } from '../lib/api';
  import { joinedLabel, relativeTime } from '../lib/models';
  import type { BookRecord, ProfilePost, ProfileRecord, ProfileRepost } from '../lib/models';
  import { profileTab, profileUrl, readerUrl, router } from '../lib/router.svelte';
  import type { ProfileTab } from '../lib/router.svelte';
  import { restoreScroll } from '../lib/screen';
  import type { FeedScreenProps } from '../lib/screen';
  import { store, message } from '../lib/store.svelte';

  let { username, scroller, oncomment, onprofile, oncount }: FeedScreenProps & {
    username: string;
    onprofile: (username: string) => void;
    oncount: (count: number | null) => void;
  } = $props();

  const tabs = [
    { id: 'posts', label: 'Posts' },
    { id: 'reposts', label: 'Reposts' },
    { id: 'books', label: 'Books' },
  ];

  let record = $state<ProfileRecord | null>(null);
  let missing = $state(false);
  let posts = $state<ProfilePost[]>([]);
  let reposts = $state<ProfileRepost[]>([]);
  let books = $state<BookRecord[]>([]);
  let postsCursor = $state<string | null>(null);
  let repostsCursor = $state<string | null>(null);
  let loading = $state(true);
  let listLoading = $state(false);
  let more = $state(false);
  let error = $state('');
  let tab = $derived(profileTab(router.url));
  let loadedTab = $state<ProfileTab | null>(null);
  let active = true;
  let recordRequest = 0;
  let listRequest = 0;
  let abort: AbortController | undefined;

  function select(id: string): void {
    if (id !== 'posts' && id !== 'reposts' && id !== 'books') return;
    if (id === tab) return;
    router.go(profileUrl(username, id), true);
  }

  async function loadTab(kind: ProfileTab, cursor: string | null, signal: AbortSignal): Promise<void> {
    if (kind === 'posts') {
      const page = await store.profilePosts(username, cursor, signal);
      if (signal.aborted) return;
      posts = cursor ? [...posts, ...page.items] : page.items;
      postsCursor = page.nextCursor;
    } else if (kind === 'reposts') {
      const page = await store.profileReposts(username, cursor, signal);
      if (signal.aborted) return;
      reposts = cursor ? [...reposts, ...page.items] : page.items;
      repostsCursor = page.nextCursor;
    } else {
      const page = await store.profileBooks(username, signal);
      if (signal.aborted) return;
      books = page.items;
    }
  }

  async function switchTab(kind: ProfileTab): Promise<void> {
    const latest = ++listRequest;
    const token = store.session?.token;
    abort?.abort();
    const signal = new AbortController();
    abort = signal;
    more = false;
    listLoading = true;
    error = '';
    try {
      await loadTab(kind, null, signal.signal);
      if (!active || latest !== listRequest || token !== store.session?.token || signal.signal.aborted) return;
      loadedTab = kind;
      const ids = [...new Set([
        ...posts.map((row) => row.book.id),
        ...reposts.map((row) => row.book.id),
      ])];
      for (const id of ids) void store.social(id).catch(() => {});
    } finally {
      if (active && latest === listRequest) listLoading = false;
    }
  }

  // The header stays. A tab change, or a pull to refresh, only reloads the list.
  $effect(() => {
    const kind = tab;
    if (!record) return;
    untrack(() => {
      if (kind === loadedTab) return;
      void switchTab(kind).catch((err: unknown) => {
        if (active) error = message(err);
      });
    });
  });

  export async function refresh(): Promise<void> {
    const latest = ++recordRequest;
    const token = store.session?.token;
    abort?.abort();
    listRequest += 1;
    try {
      const next = await store.profileRecord(username);
      if (!active || latest !== recordRequest || token !== store.session?.token) return;
      loadedTab = null;
      record = next;
      missing = false;
      oncount(next.postCount);
    } catch (err) {
      if (!active || latest !== recordRequest || token !== store.session?.token) return;
      if (err instanceof ApiError && err.status === 404) {
        missing = true;
        record = null;
        oncount(null);
        return;
      }
      throw err;
    }
  }

  async function load(): Promise<void> {
    loading = true;
    error = '';
    try {
      await refresh();
      if (!active) return;
      loading = false;
      await tick();
      if (active) restoreScroll(scroller);
    } catch (err) {
      if (active) { error = message(err); loading = false; }
    }
  }

  async function loadMore(): Promise<void> {
    const kind = tab;
    const cursor = kind === 'posts' ? postsCursor : kind === 'reposts' ? repostsCursor : null;
    if (!cursor || more || loading || listLoading) return;
    const latest = listRequest;
    const token = store.session?.token;
    more = true;
    abort?.abort();
    const signal = new AbortController();
    abort = signal;
    try {
      await loadTab(kind, cursor, signal.signal);
    } catch (err) {
      if (active && latest === listRequest && token === store.session?.token && !signal.signal.aborted) {
        error = message(err);
      }
    } finally {
      if (active && latest === listRequest) more = false;
    }
  }

  function sentinel(node: HTMLElement): { destroy: () => void } {
    const observer = new IntersectionObserver((entries) => {
      if (entries.some((entry) => entry.isIntersecting)) void loadMore();
    }, { root: scroller });
    observer.observe(node);
    return { destroy: () => observer.disconnect() };
  }

  onMount(() => {
    void load();
    return () => { active = false; abort?.abort(); };
  });

  let cursor = $derived(tab === 'posts' ? postsCursor : tab === 'reposts' ? repostsCursor : null);
  let empty = $derived(!loading && !listLoading && !error && !missing && (
    (tab === 'posts' && !posts.length) || (tab === 'reposts' && !reposts.length) || (tab === 'books' && !books.length)
  ));
</script>

{#if missing}
  <p class="inline-status">This account doesn't exist</p>
{:else if loading && !record}
  <div class="status-row" role="status">
    <span class="spinner"></span>
    <span class="sr-only">Loading profile…</span>
  </div>
{:else if error && !record}
  <div class="status-row">
    <span class="error" role="alert">{error}</span>
    <button class="text-button" onclick={load}>Try again</button>
  </div>
{:else if record}
  <section class="profile-header" aria-label={`${record.username}'s profile`}>
    <div class="profile-banner"></div>
    <div class="profile-avatar"><Avatar name={record.username} size={68} /></div>
    <div class="profile-info">
      <h2>{record.username}</h2>
      <p class="meta">@{record.username}</p>
      <p class="meta">{joinedLabel(record.joinedAt)}</p>
    </div>
  </section>
  <UnderlineTabs {tabs} selected={tab} onselect={select} />
  {#if error}
    <div class="status-row">
      <span class="error" role="alert">{error}</span>
      <button class="text-button" onclick={load}>Try again</button>
    </div>
  {/if}
  {#if listLoading}
    <div class="status-row" role="status">
      <span class="spinner"></span>
      <span class="sr-only">Loading profile…</span>
    </div>
  {:else}
    {#if tab === 'posts'}
      {#each posts as reply (reply.comment.id)}
        <div class="profile-reply">
          <Post
            book={reply.book}
            post={reply.post}
            connected
            onopen={() => router.go(readerUrl(reply.book.id, reply.post.id))}
            oncomment={() => oncomment(reply.book, reply.post)}
          />
          <ThreadRow
            name={reply.comment.username}
            handle={reply.comment.username}
            meta={relativeTime(reply.comment.createdAt)}
            text={reply.comment.text}
            {onprofile}
            onopen={() => oncomment(reply.book, reply.post)}
          />
        </div>
      {/each}
    {:else if tab === 'reposts'}
      {#each reposts as hit (`${hit.book.id}:${hit.post.id}`)}
        <Post
          book={hit.book}
          post={hit.post}
          onopen={() => router.go(readerUrl(hit.book.id, hit.post.id))}
          oncomment={() => oncomment(hit.book, hit.post)}
        />
      {/each}
    {:else}
      {#each books as book (book.id)}
        <BookRow {book} />
      {/each}
    {/if}
    {#if more}
      <div class="status-row" role="status"><span class="spinner"></span></div>
    {/if}
    {#if cursor && !more}
      <div use:sentinel></div>
    {/if}
    {#if empty}
      <p class="inline-status">{tab === 'books' ? 'No books' : 'No posts'}</p>
    {/if}
  {/if}
{/if}

<style>
.profile-banner {
  height: 125px;
  background: var(--hover);
}

.profile-header {
  position: relative;
}

.profile-avatar {
  position: absolute;
  left: 12px;
  top: 87px;
  width: 76px;
  height: 76px;
  background: var(--ground);
  border-radius: 50%;
  padding: 4px;
}

.profile-info {
  display: flex;
  flex-direction: column;
  gap: 2px;
  padding: 42px 16px 16px;
}

.profile-info h2 {
  font-size: 1.25rem;
  line-height: 1.2;
}

.profile-reply {
  border-bottom: .6px solid var(--line);
}

.profile-reply :global(.post) {
  border-bottom: 0;
}

.profile-reply :global(.thread-row) {
  padding-bottom: 16px;
}
</style>
