<script lang="ts">
  import Avatar from './Avatar.svelte';
  import Icon from './Icon.svelte';
  import PostText from './PostText.svelte';
  import { profileUrl, router } from '../lib/router.svelte';

  let {
    name, handle, meta, text, src = null, connected = false, heading = false,
    onprofile, ondelete, onopen,
  }: {
    name: string;
    handle: string;
    meta: string;
    text: string;
    src?: string | null;
    connected?: boolean;
    heading?: boolean;
    onprofile?: (username: string) => void;
    ondelete?: () => void;
    onopen?: () => void;
  } = $props();
  let menu = $state(false);

  function profile(event: MouseEvent): void {
    event.preventDefault();
    if (onprofile) onprofile(handle);
    else router.go(profileUrl(handle));
  }

  function openRow(event: MouseEvent): void {
    const target = event.target;
    if (target instanceof Element && target.closest('a, button')) return;
    onopen?.();
  }

  function openRowKey(event: KeyboardEvent): void {
    if (event.target !== event.currentTarget) return;
    if (event.key !== 'Enter' && event.key !== ' ') return;
    event.preventDefault();
    onopen?.();
  }
</script>

<div class="thread-row">
  <div class="avatar-column">
    {#if onprofile}
      <a
        class="avatar-link"
        href={profileUrl(handle)}
        aria-label={`Open ${name}'s profile`}
        onclick={profile}
      >
        <Avatar {name} {src} />
      </a>
    {:else}
      <Avatar {name} {src} />
    {/if}
    {#if connected}<span class="thread-rule"></span>{/if}
  </div>
  <div class="post-column">
    <div class="post-meta">
      {#if onprofile}
        <a class="identity truncate" href={profileUrl(handle)} onclick={profile}>
          <strong>{name}</strong>
          <span class="meta">@{handle} · {meta}</span>
        </a>
      {:else}
        <strong class="post-name">{name}</strong>
        <span class="meta truncate">@{handle} · {meta}</span>
      {/if}
      {#if ondelete}
        <div class="reply-menu">
          <button
            type="button"
            class="icon-button"
            aria-label="More reply actions"
            aria-expanded={menu}
            onclick={() => menu = !menu}
          >
            <Icon name="more" size={18} />
          </button>
          {#if menu}
            <button
              type="button"
              class="delete-menu danger"
              onclick={() => { menu = false; ondelete?.(); }}
            >
              Delete reply
            </button>
          {/if}
        </div>
      {/if}
    </div>
    {#if onopen}
      <div
        class="post-body body-button"
        class:heading
        role="button"
        tabindex="0"
        onclick={openRow}
        onkeydown={openRowKey}
      >
        <PostText {text} />
      </div>
    {:else}
      <div class="post-body" class:heading><PostText {text} /></div>
    {/if}
  </div>
</div>

<style>
.avatar-link {
  display: inline-flex;
  border-radius: 50%;
}

.thread-row {
  padding-bottom: 0;
}

.thread-row .post-column {
  padding-bottom: 12px;
}

.thread-row .post-meta {
  min-height: 24px;
}

.identity {
  flex: 1;
  font-size: 1.0625rem;
}

.identity:hover strong {
  text-decoration: underline;
}

.body-button {
  display: block;
  text-align: left;
  width: 100%;
  cursor: pointer;
}

.reply-menu {
  position: relative;
  margin-top: -8px;
  margin-right: -4px;
}

.delete-menu {
  position: absolute;
  top: 100%;
  right: 0;
  white-space: nowrap;
  padding: 12px 16px;
  background: var(--ground);
  border: 1px solid var(--line);
  z-index: 2;
}

.delete-menu:hover {
  background: var(--hover);
}


</style>
