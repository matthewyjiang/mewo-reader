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
    Account
    <Icon name="chevron" />
  </button>
  <button class="settings-row" onclick={() => router.go('/settings/display')}>
    Display
    <Icon name="chevron" />
  </button>
  <button class="settings-row" onclick={() => router.go('/settings/library')}>
    Library
    <Icon name="chevron" />
  </button>
  {#if store.me?.canRequest}
    <button class="settings-row" onclick={() => router.go('/settings/requests')}>
      Requests
      <Icon name="chevron" />
    </button>
  {/if}
  {#if store.me?.isAdmin}
    <button class="settings-row" onclick={() => router.go('/settings/server')}>
      Server
      <Icon name="chevron" />
    </button>
  {/if}
  <button class="settings-row" onclick={() => router.go('/settings/about')}>
    About
    <Icon name="chevron" />
  </button>
{:else if path === '/settings/display'}
  <DisplaySettings />
{:else if path === '/settings/account' && store.session}
  <div class="account-line">
    <Avatar name={store.session.username} size={40} />
    <strong>{store.session.username}</strong>
  </div>
  <button class="settings-row" onclick={() => onprofile(store.session!.username)}>
    View profile
  </button>
  <button class="settings-row danger" onclick={onsignout}>Sign out</button>
{:else if path === '/settings/library'}
  <div class="fact-row">
    <span>Server URL</span>
    <span>{location.origin}</span>
  </div>
  <button class="settings-row" disabled={uploading} onclick={onadd}>Add an EPUB</button>
  <button class="settings-row" disabled={uploading} onclick={onsample}>Open a sample</button>
{:else if path === '/settings/about'}
  <section aria-label="App info">
    <div class="fact-row">
      <span>Web app</span>
      <span>{formatBuild(webVersion, webCommit)}</span>
    </div>
    <div class="fact-row">
      <span>Server</span>
      <span class:muted={serverBuild.status !== 'ok'}>{serverLabel()}</span>
    </div>
    <div class="fact-row">
      <span>Server URL</span>
      <span>{location.origin}</span>
    </div>
  </section>
{:else}
  <div class="empty-state">
    <h2>Not found</h2>
    <button class="pill" onclick={() => router.go('/settings')}>Back to Settings</button>
  </div>
{/if}

<style>
.settings-row {
  width: 100%;
  min-height: 52px;
  padding: 0 16px;
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 16px;
  text-align: left;
  border-bottom: .6px solid var(--line);
  font-size: 1.0625rem;
  font-weight: 700;
}

.settings-row :global(svg) {
  color: var(--mute);
  flex-shrink: 0;
}

.settings-row:hover:not(:disabled) {
  background: var(--hover);
}

.settings-row.danger {
  color: var(--like);
}

.account-line {
  min-height: 64px;
  display: flex;
  align-items: center;
  gap: 12px;
  padding: 0 16px;
  border-bottom: .6px solid var(--line);
}

.account-line strong {
  font-size: 1.0625rem;
  overflow-wrap: anywhere;
}

.fact-row {
  min-height: 52px;
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 16px;
  padding: 8px 16px;
  border-bottom: .6px solid var(--line);
}

.fact-row > :first-child {
  font-weight: 700;
  font-size: 1.0625rem;
  flex: 0 0 auto;
}

.fact-row > :last-child {
  text-align: right;
  overflow-wrap: anywhere;
}
</style>
