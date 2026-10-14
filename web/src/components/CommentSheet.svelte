<script lang="ts">
  import { onMount } from 'svelte';
  import type { PostComment, PostHit } from '../lib/models';
  import { relativeTime } from '../lib/models';
  import { store, message } from '../lib/store.svelte';
  import Sheet from './Sheet.svelte';
  import ThreadRow from './ThreadRow.svelte';
  import Avatar from './Avatar.svelte';
  import Icon from './Icon.svelte';

  let { target, onclose, onprofile, onchanged }: {
    target: PostHit;
    onclose: () => void;
    onprofile: (username: string) => void;
    onchanged?: () => void;
  } = $props();

  let comments = $state<PostComment[]>([]);
  let loading = $state(true);
  let draft = $state('');
  let focused = $state(false);
  let sending = $state(false);
  let deleting = $state(false);
  let error = $state('');
  let active = true;
  const token = store.session?.token;
  // Server and phone comment budget, measured in Unicode characters.
  const budget = 2000;
  let characters = $derived(Array.from(draft.trim()).length);
  let budgetError = $derived(characters > budget
    ? `Comment budget: asked ${characters} characters, limit is ${budget}.`
    : draft && !draft.trim() ? 'Write something first.' : '');

  function publish(next: PostComment[]): void {
    if (!active || store.session?.token !== token) return;
    comments = next;
    onchanged?.();
  }

  onMount(() => {
    store.comments(target.book.id, target.post.id).then((next) => {
      if (active) publish(next);
    }).catch((err: unknown) => {
      if (active) error = message(err);
    }).finally(() => {
      if (active) loading = false;
    });
    return () => { active = false; };
  });

  async function reply(event: SubmitEvent): Promise<void> {
    event.preventDefault();
    if (!draft.trim()) { error = 'Write something first.'; return; }
    if (characters > budget) { error = budgetError; return; }
    sending = true;
    error = '';
    try {
      const next = await store.addComment(target.book.id, target.post.id, draft.trim());
      publish(next);
      if (!active) return;
      draft = '';
      focused = false;
    } catch (err) {
      if (active) error = message(err);
    } finally {
      if (active) sending = false;
    }
  }

  async function remove(comment: PostComment): Promise<void> {
    if (deleting) return;
    deleting = true;
    try {
      publish(await store.deleteComment(target.book.id, target.post.id, comment.id));
    } catch (err) {
      if (active) error = message(err);
    } finally {
      if (active) deleting = false;
    }
  }
</script>

<Sheet label="Notes on a line" kind="full" {onclose}>
  <form class="comment-form" onsubmit={reply}>
    <header class="sheet-bar">
      <button
        type="button"
        class="icon-button"
        aria-label="Close replies"
        onclick={onclose}
      >
        <Icon name="close" />
      </button>
      <h2 class="sr-only">Notes on a line</h2>
      <button
        class="pill"
        type="submit"
        disabled={!draft.trim() || characters > budget || sending || loading}
      >
        {sending ? 'Replying…' : 'Reply'}
      </button>
    </header>
    <div class="comment-scroll">
      <ThreadRow
        name={target.book.author}
        handle={target.book.handle}
        meta={target.post.chapter}
        text={target.post.text}
        src={store.covers[target.book.id]}
        heading={target.post.kind === 'Heading'}
        connected
      />
      {#if loading}<p class="inline-status" role="status">Loading replies…</p>{/if}
      {#each comments as comment (comment.id)}
        <ThreadRow
          name={comment.username}
          handle={comment.username}
          meta={relativeTime(comment.createdAt)}
          text={comment.text}
          connected
          onprofile={(username) => { onclose(); onprofile(username); }}
          ondelete={comment.mine && !deleting ? () => remove(comment) : undefined}
        />
      {/each}
      <div class="compose-row">
        <Avatar name={store.session?.username ?? 'M'} />
        <div class="post-column">
          {#if focused || draft}
            <p class="meta accent">Replying to @{target.book.handle}</p>
          {/if}
          <label class="sr-only" for="reply-draft">Reply, public on this server</label>
          <textarea
            id="reply-draft"
            rows="8"
            placeholder="Post your reply"
            bind:value={draft}
            disabled={sending}
            onfocus={() => focused = true}
            onblur={() => focused = false}
            oninput={() => error = ''}
            aria-describedby="reply-budget reply-error"
          ></textarea>
          <div class="composer-meta">
            <span class="meta">Public on this server</span>
            <span id="reply-budget" class:danger={characters > budget} class="meta">
              {characters} / {budget}
            </span>
          </div>
          <p id="reply-error" class="error" role="alert">{error || budgetError}</p>
        </div>
      </div>
    </div>
  </form>
</Sheet>

<style>
.comment-form {
  height: 100%;
  display: flex;
  flex-direction: column;
}

.comment-scroll {
  overflow-y: auto;
  min-height: 0;
  flex: 1;
  overscroll-behavior: contain;
}

.compose-row {
  display: flex;
  gap: 12px;
  padding: 8px 16px 24px;
}

.compose-row textarea {
  width: 100%;
  resize: vertical;
  border: 0;
  background: transparent;
  color: var(--ink);
  padding: 8px 0;
  font-size: 1.0625rem;
  line-height: 1.41;
}

.compose-row .meta.accent {
  margin-top: 4px;
}

.composer-meta {
  display: flex;
  justify-content: space-between;
  gap: 8px;
  margin-bottom: 8px;
}


</style>
