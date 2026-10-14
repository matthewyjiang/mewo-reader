<script lang="ts">
  import Avatar from './Avatar.svelte';
  import Icon from './Icon.svelte';
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
      <button type="button" class="post-body body-button" class:heading onclick={onopen}>
        {text}
      </button>
    {:else}
      <div class="post-body" class:heading>{text}</div>
    {/if}
  </div>
</div>
