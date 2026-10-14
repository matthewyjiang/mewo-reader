<script lang="ts">
  import { onMount } from 'svelte';
  import Avatar from '../components/Avatar.svelte';
  import DisplaySettings from '../components/DisplaySettings.svelte';
  import Icon from '../components/Icon.svelte';
  import { webCommit, webVersion } from '../lib/build';
  import { router } from '../lib/router.svelte';
  import { restoreScroll } from '../lib/screen';
  import { store } from '../lib/store.svelte';

  type ServerBuild =
    | { status: 'loading' }
    | { status: 'error' }
    | { status: 'ok'; version: string; commit: string | null };

  let serverBuild = $state<ServerBuild>({ status: 'loading' });

  function formatBuild(version: string, commit: string | null): string {
    return commit ? `${version} (${commit})` : version;
  }

  function serverLabel(): string {
    if (serverBuild.status === 'loading') return 'Loading…';
    if (serverBuild.status === 'error') return 'Unavailable';
    return formatBuild(serverBuild.version, serverBuild.commit);
  }

  function readServerVersion(body: unknown): ServerBuild | null {
    if (!body || typeof body !== 'object' || !('version' in body)) return null;
    if (typeof body.version !== 'string' || body.version.length === 0) return null;
    const commit = 'commit' in body && typeof body.commit === 'string' && body.commit.length > 0
      ? body.commit
      : null;
    return { status: 'ok', version: body.version, commit };
  }

  let { path, scroller, uploading, onadd, onsample, onprofile, onsignout }: {
    path: string;
    scroller: HTMLElement;
    uploading: boolean;
    onadd: () => void;
    onsample: () => void;
    onprofile: (username: string) => void;
    onsignout: () => void;
  } = $props();

  $effect(() => {
    if (path !== '/settings/about') return;
    const controller = new AbortController();
    serverBuild = { status: 'loading' };
    void fetch('/v1/version', { signal: controller.signal })
      .then(async (response) => {
        if (!response.ok) throw new Error(String(response.status));
        return readServerVersion(await response.json());
      })
      .then((info) => {
        if (controller.signal.aborted) return;
        serverBuild = info ?? { status: 'error' };
      })
      .catch(() => {
        if (!controller.signal.aborted) serverBuild = { status: 'error' };
      });
    return () => controller.abort();
  });

  onMount(() => restoreScroll(scroller));
</script>

{#if path === '/settings'}
  <button class="settings-row" onclick={() => router.go('/settings/account')}>
    <span>
      <strong>Account</strong>
      <span class="meta">@{store.session?.username}</span>
    </span>
    <Icon name="chevron" />
  </button>
  <button class="settings-row" onclick={() => router.go('/settings/display')}>
    <span>
      <strong>Display</strong>
      <span class="meta">Twitter or X. Light or night. Font size. Mewo mode.</span>
    </span>
    <Icon name="chevron" />
  </button>
  <button class="settings-row" onclick={() => router.go('/settings/library')}>
    <span>
      <strong>Library</strong>
      <span class="meta">Private until you share a book.</span>
    </span>
    <Icon name="chevron" />
  </button>
  <button class="settings-row" onclick={() => router.go('/settings/about')}>
    <span>
      <strong>About</strong>
      <span class="meta">Web and server versions.</span>
    </span>
    <Icon name="chevron" />
  </button>
{:else if path === '/settings/display'}
  <DisplaySettings />
{:else if path === '/settings/account' && store.session}
  <section class="settings-content">
    <div class="identity-preview">
      <Avatar name={store.session.username} />
      <div>
        <strong>{store.session.username}</strong>
        <p class="meta">@{store.session.username}</p>
      </div>
    </div>
    <p class="muted">
      Your hosted username is used on your profile and notes.
      Account names are managed by the server.
    </p>
    <button class="text-button" onclick={() => onprofile(store.session!.username)}>
      View profile
    </button>
    <button class="text-button danger" onclick={onsignout}>Sign out</button>
  </section>
{:else if path === '/settings/library'}
  <section class="settings-content">
    <h2>Hosted library</h2>
    <p class="muted">
      Books you add stay private until you share them. Other accounts can read a
      book you share, and the sample. Notes are visible to anyone who can read
      the book. Likes and reading place stay yours. Only the person who added a
      book can remove it or change sharing.
    </p>
    <p class="meta">{location.origin}</p>
    <button class="pill" disabled={uploading} onclick={onadd}>Add an EPUB</button>
    <button class="text-button" disabled={uploading} onclick={onsample}>Open a sample</button>
  </section>
{:else if path === '/settings/about'}
  <section class="settings-content about" aria-label="App info">
    <dl>
      <div>
        <dt>Web app</dt>
        <dd>{formatBuild(webVersion, webCommit)}</dd>
      </div>
      <div>
        <dt>Server</dt>
        <dd class:muted={serverBuild.status !== 'ok'}>{serverLabel()}</dd>
      </div>
      <div>
        <dt>Server URL</dt>
        <dd>{location.origin}</dd>
      </div>
    </dl>
  </section>
{:else}
  <div class="empty-state">
    <h2>That page isn't here</h2>
    <button class="pill" onclick={() => router.go('/settings')}>Back to Settings</button>
  </div>
{/if}

<style>
.settings-row {
  width: 100%;
  padding: 20px 16px;
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 16px;
  text-align: left;
  border-bottom: .6px solid var(--line);
}

.settings-row > span {
  display: flex;
  flex-direction: column;
  gap: 4px;
}

.settings-row strong {
  font-size: 1.0625rem;
}

.settings-row :global(svg) {
  color: var(--mute);
  flex-shrink: 0;
}

.settings-row:hover {
  background: var(--hover);
}

.settings-content {
  padding: 24px 16px;
  display: flex;
  flex-direction: column;
  align-items: flex-start;
  gap: 16px;
}

.about dl {
  width: 100%;
  margin: 0;
  display: flex;
  flex-direction: column;
  gap: 16px;
}

.about div {
  display: flex;
  flex-direction: column;
  gap: 4px;
}

.about dt {
  color: var(--mute);
  font-size: .8125rem;
  line-height: 1.23;
}

.about dd {
  margin: 0;
  overflow-wrap: anywhere;
}


</style>
