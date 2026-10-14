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
  let ownProfile = $derived(
    username.toLowerCase() === store.session?.username.toLowerCase(),
  );

  export async function refresh(): Promise<void> {
    const token = store.session?.token;
    const next = await store.profile(username);
    if (!active || token !== store.session?.token) return;
    replies = next;
    oncount(next.length);
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
      <p class="meta">@{username}</p>
    </div>
    <div class="posts-tab">Posts</div>
  </section>
  {#if !replies.length}
    <p class="inline-status">
      {ownProfile ? 'Reply to a line. It shows up here.' : 'No posts yet.'}
    </p>
  {:else}
    {#each replies as reply (reply.comment.id)}
      <div class="profile-reply timeline-item">
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
