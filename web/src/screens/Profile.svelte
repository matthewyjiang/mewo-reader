<script lang="ts">
  import { onMount, tick } from 'svelte';
  import Avatar from '../components/Avatar.svelte';
  import FeedStatus from '../components/FeedStatus.svelte';
  import Post from '../components/Post.svelte';
  import ThreadRow from '../components/ThreadRow.svelte';
  import { chrome } from '../lib/chrome.svelte';
  import { relativeTime } from '../lib/models';
  import type { ProfileReply } from '../lib/models';
  import { restoreScroll } from '../lib/screen';
  import type { FeedScreenProps } from '../lib/screen';
  import { store, message } from '../lib/store.svelte';

  let { username, scroller, oncomment, onprofile, oncount }: FeedScreenProps & {
    username: string;
    onprofile: (username: string) => void;
    oncount: (count: number) => void;
  } = $props();

  let replies = $state<ProfileReply[]>([]);
  let loading = $state(true);
  let error = $state('');
  let active = true;
  let request = 0;

  export async function refresh(): Promise<void> {
    const latest = ++request;
    const token = store.session?.token;
    try {
      const next = await store.profile(username);
      if (!active || latest !== request || token !== store.session?.token) return;
      replies = next;
      oncount(next.length);
    } catch (error) {
      if (active && latest === request && token === store.session?.token) throw error;
    }
  }

  async function load(): Promise<void> {
    loading = true;
    error = '';
    chrome.show();
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

  onMount(() => {
    void load();
    return () => { active = false; };
  });
</script>

{#if loading || error}
  <FeedStatus {loading} {error} label="profile" onretry={load} />
{:else}
  <section class="profile-header" aria-label={`${username}'s profile`}>
    <div class="profile-banner"></div>
    <div class="profile-avatar"><Avatar name={username} size={68} /></div>
    <div class="profile-info">
      <h2>{username}</h2>
    </div>
    <div class="posts-tab">Posts</div>
  </section>
  {#if !replies.length}
    <p class="inline-status">
      No posts
    </p>
  {:else}
    {#each replies as reply (reply.comment.id)}
      <div class="profile-reply">
        <Post
          book={reply.book}
          post={reply.post}
          connected
          onopen={() => oncomment(reply.book, reply.post)}
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
  padding: 42px 16px 16px;
}

.posts-tab {
  height: 48px;
  display: flex;
  align-items: center;
  justify-content: center;
  font-weight: 700;
  position: relative;
  border-bottom: .6px solid var(--line);
}

.posts-tab::after {
  content: '';
  position: absolute;
  bottom: 0;
  height: 3px;
  width: 56px;
  border-radius: 999px;
  background: var(--accent);
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

@media (forced-colors: active) {
  .posts-tab::after {
    background: CanvasText;
  }
}
</style>
