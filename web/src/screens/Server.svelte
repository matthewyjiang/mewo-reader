<script lang="ts">
  import { onMount } from 'svelte';
  import Avatar from '../components/Avatar.svelte';
  import Icon from '../components/Icon.svelte';
  import { ApiError } from '../lib/api';
  import type { AdminUser, ServerInfo, ServerSettings } from '../lib/models';
  import { router } from '../lib/router.svelte';
  import { restoreScroll } from '../lib/screen';
  import { bytesToMb, formatBytes, parseMb } from '../lib/server';
  import { message, store } from '../lib/store.svelte';

  let { path, scroller, onprofile }: {
    path: string;
    scroller: HTMLElement;
    onprofile: (username: string) => void;
  } = $props();

  let status = $state<'loading' | 'ready' | 'denied'>('loading');
  let settings = $state<ServerSettings | null>(null);
  let info = $state<ServerInfo | null>(null);
  let users = $state<AdminUser[] | null>(null);
  let mb = $state('');
  let saving = $state(false);
  let generation = 0;

  function mbField(bytes: number): string {
    const value = bytesToMb(bytes);
    if (Number.isInteger(value)) return String(value);
    return String(Math.round(value * 1000) / 1000);
  }

  $effect(() => {
    const current = path;
    const ticket = ++generation;
    status = 'loading';
    settings = null;
    info = null;
    users = null;
    void load(current, ticket);
  });

  async function load(current: string, ticket: number): Promise<void> {
    try {
      if (current === '/settings/server/users') {
        const items = await store.api.adminUsers();
        if (ticket !== generation) return;
        users = items;
        status = 'ready';
        return;
      }
      const [nextSettings, nextInfo] = await Promise.all([
        store.api.serverSettings(),
        store.api.serverInfo(),
      ]);
      if (ticket !== generation) return;
      settings = nextSettings;
      info = nextInfo;
      mb = mbField(nextSettings.maxEpubBytes);
      status = 'ready';
    } catch (error) {
      if (ticket !== generation) return;
      if (error instanceof ApiError && error.status === 403) {
        status = 'denied';
        return;
      }
      store.notice = message(error);
      status = 'denied';
    }
  }

  async function setSignup(allow: boolean): Promise<void> {
    if (!settings || saving || allow === settings.allowSignup) return;
    const previous = settings;
    settings = { ...settings, allowSignup: allow };
    saving = true;
    try {
      settings = await store.api.updateServerSettings({ allowSignup: allow });
    } catch (error) {
      settings = previous;
      store.notice = message(error);
    } finally {
      saving = false;
    }
  }

  async function saveMb(): Promise<void> {
    if (!settings || saving) return;
    const bytes = parseMb(mb);
    if (bytes === null || bytes === settings.maxEpubBytes) {
      mb = mbField(settings.maxEpubBytes);
      return;
    }
    saving = true;
    try {
      settings = await store.api.updateServerSettings({ maxEpubBytes: bytes });
      mb = mbField(settings.maxEpubBytes);
    } catch (error) {
      mb = mbField(settings.maxEpubBytes);
      store.notice = message(error);
    } finally {
      saving = false;
    }
  }

  async function toggleAdmin(user: AdminUser): Promise<void> {
    if (user.mine || !users) return;
    try {
      const saved = await store.api.setAdmin(user.username, !user.isAdmin);
      users = users.map((row) => row.username === saved.username ? saved : row);
    } catch (error) {
      store.notice = message(error);
    }
  }

  onMount(() => restoreScroll(scroller));
</script>

{#if status === 'denied'}
  <div class="empty-state">
    <h2>Not found</h2>
    <button class="pill" onclick={() => router.go('/settings')}>Back to Settings</button>
  </div>
{:else if status === 'ready' && path === '/settings/server/users' && users}
  {#each users as user (user.username)}
    <div class="user-row">
      <Avatar name={user.username} size={40} />
      <span class="account-names">
        <strong>{user.username}</strong>
        <button class="handle" onclick={() => onprofile(user.username)}>@{user.username}</button>
      </span>
      <button
        class="icon-button shield"
        aria-pressed={user.isAdmin}
        aria-label={`${user.username} admin`}
        disabled={user.mine}
        onclick={() => toggleAdmin(user)}
      >
        <Icon name="shield" filled={user.isAdmin} size={20} />
      </button>
    </div>
  {/each}
{:else if status === 'ready' && settings && info}
  <label class="switch-row">
    <span>Signups</span>
    <input
      type="checkbox"
      role="switch"
      checked={settings.allowSignup}
      disabled={saving}
      onchange={(event) => setSignup(event.currentTarget.checked)}
    />
  </label>
  <label class="settings-row">
    <span>Max EPUB size</span>
    <span class="epub-field">
      <input
        value={mb}
        inputmode="decimal"
        autocomplete="off"
        spellcheck="false"
        disabled={saving}
        oninput={(event) => mb = event.currentTarget.value}
        onblur={() => saveMb()}
        onkeydown={(event) => {
          if (event.key === 'Enter') {
            event.preventDefault();
            void saveMb();
          }
        }}
      />
      <span class="unit">MB</span>
    </span>
  </label>
  <button class="settings-row" onclick={() => router.go('/settings/server/users')}>
    Users
    <Icon name="chevron" />
  </button>
  <div class="fact-row">
    <span>Users</span>
    <span>{info.users}</span>
  </div>
  <div class="fact-row">
    <span>Books</span>
    <span>{info.books}</span>
  </div>
  <div class="fact-row">
    <span>Comments</span>
    <span>{info.comments}</span>
  </div>
  <div class="fact-row">
    <span>Storage</span>
    <span>{formatBytes(info.storageBytes)}</span>
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

.switch-row {
  display: flex;
  align-items: center;
  gap: 16px;
  justify-content: space-between;
  min-height: 52px;
  padding: 0 16px;
  border-bottom: .6px solid var(--line);
  font-weight: 700;
  font-size: 1.0625rem;
}

.switch-row > span {
  flex: 1;
  min-width: 0;
}

.switch-row input {
  width: 36px;
  height: 24px;
  flex: 0 0 36px;
  cursor: pointer;
}

.epub-field {
  display: inline-flex;
  align-items: center;
  gap: 8px;
  font-weight: 400;
}

.epub-field input {
  width: 5.5rem;
  height: 36px;
  padding: 0 8px;
  text-align: right;
  color: var(--ink);
  background: var(--ground);
  border: 1px solid var(--line);
  border-radius: 4px;
  font: inherit;
}

.unit {
  color: var(--mute);
  font-size: .9375rem;
  font-weight: 400;
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

.user-row {
  display: flex;
  align-items: center;
  gap: 12px;
  min-height: 64px;
  padding: 8px 8px 8px 16px;
  border-bottom: .6px solid var(--line);
}

.user-row .account-names {
  flex: 1;
  min-width: 0;
}

.user-row .account-names strong {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.handle {
  color: var(--mute);
  font-size: .8125rem;
  font-weight: 400;
  line-height: 1.23;
  text-align: left;
}

.handle:hover {
  text-decoration: underline;
}

.shield[aria-pressed='true'] {
  color: var(--accent);
}

.shield:not([aria-pressed='true']) {
  color: var(--mute);
}
</style>
