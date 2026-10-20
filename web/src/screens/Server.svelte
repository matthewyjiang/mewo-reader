<script lang="ts">
  import { onMount } from 'svelte';
  import Avatar from '../components/Avatar.svelte';
  import Icon from '../components/Icon.svelte';
  import { ApiError } from '../lib/api';
  import type { AdminUser, ServerInfo, ServerSettings } from '../lib/models';
  import { router } from '../lib/router.svelte';
  import { restoreScroll } from '../lib/screen';
  import { bytesToMb, epubMbError, formatBytes, parseMb } from '../lib/server';
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
  let generation = 0;
  let pending: Promise<void> = Promise.resolve();
  const queuedText: Record<string, string> = {};

  function enqueue(run: () => Promise<void>): void {
    pending = pending.then(run, run);
  }
  let prowlarrUrl = $state('');
  let sabUrl = $state('');
  let category = $state('books');
  let prowlarrKey = $state('');
  let sabKey = $state('');
  let checks = $state<{ name: string; ok: boolean; detail: string }[] | null>(null);
  let testing = $state(false);
  let requestCount = $state<number | null>(null);

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
      prowlarrUrl = nextSettings.prowlarrUrl ?? '';
      sabUrl = nextSettings.sabnzbdUrl ?? '';
      category = nextSettings.sabnzbdCategory || 'books';
      prowlarrKey = '';
      sabKey = '';
      delete queuedText.prowlarrUrl;
      delete queuedText.sabnzbdUrl;
      delete queuedText.sabnzbdCategory;
      delete queuedText.maxEpubBytes;
      status = 'ready';
      void loadRequestCount(nextSettings.requestsEnabled === true);
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

  async function loadRequestCount(enabled: boolean): Promise<void> {
    if (!enabled) {
      requestCount = 0;
      return;
    }
    try {
      requestCount = (await store.api.requests(true)).length;
    } catch {
      requestCount = null;
    }
  }

  function saveText(
    key: 'prowlarrUrl' | 'sabnzbdUrl' | 'sabnzbdCategory',
    value: string,
  ): void {
    if (!settings) return;
    const current = queuedText[key] ?? settings[key] ?? (key === 'sabnzbdCategory' ? 'books' : '');
    if (value === current) return;
    queuedText[key] = value;
    enqueue(async () => {
      if (!settings) return;
      try {
        settings = await store.api.updateServerSettings({ [key]: value });
      } catch (error) {
        if (key === 'prowlarrUrl' && prowlarrUrl === value) {
          prowlarrUrl = settings.prowlarrUrl ?? '';
          delete queuedText.prowlarrUrl;
        }
        if (key === 'sabnzbdUrl' && sabUrl === value) {
          sabUrl = settings.sabnzbdUrl ?? '';
          delete queuedText.sabnzbdUrl;
        }
        if (key === 'sabnzbdCategory' && category === value) {
          category = settings.sabnzbdCategory || 'books';
          delete queuedText.sabnzbdCategory;
        }
        store.notice = message(error);
      }
    });
  }

  function saveKey(which: 'prowlarrApiKey' | 'sabnzbdApiKey', value: string): void {
    const trimmed = value.trim();
    if (!trimmed || !settings) return;
    enqueue(async () => {
      try {
        settings = await store.api.updateServerSettings({ [which]: trimmed });
        if (which === 'prowlarrApiKey' && prowlarrKey.trim() === trimmed) prowlarrKey = '';
        if (which === 'sabnzbdApiKey' && sabKey.trim() === trimmed) sabKey = '';
      } catch (error) {
        store.notice = message(error);
      }
    });
  }

  function clearKey(which: 'prowlarrApiKey' | 'sabnzbdApiKey'): void {
    const draft = which === 'prowlarrApiKey' ? prowlarrKey : sabKey;
    if (draft) {
      if (which === 'prowlarrApiKey') prowlarrKey = '';
      else sabKey = '';
      return;
    }
    if (!settings?.secrets?.[which]) return;
    enqueue(async () => {
      if (!settings?.secrets?.[which]) return;
      try {
        settings = await store.api.updateServerSettings({ [which]: null });
      } catch (error) {
        store.notice = message(error);
      }
    });
  }

  function setRequests(enabled: boolean): void {
    if (!settings || enabled === (settings.requestsEnabled === true)) return;
    const previous = settings;
    settings = { ...settings, requestsEnabled: enabled };
    enqueue(async () => {
      if (settings?.requestsEnabled !== enabled) return;
      try {
        const saved = await store.api.updateServerSettings({ requestsEnabled: enabled });
        if (settings?.requestsEnabled === enabled) {
          settings = saved;
          void loadRequestCount(saved.requestsEnabled === true);
        }
      } catch (error) {
        if (settings?.requestsEnabled === enabled) settings = previous;
        store.notice = message(error);
      }
    });
  }

  async function testConnection(): Promise<void> {
    if (testing) return;
    testing = true;
    try {
      checks = (await store.api.testRequests()).checks;
    } catch (error) {
      store.notice = message(error);
    } finally {
      testing = false;
    }
  }

  function checkName(name: string): string {
    if (name === 'prowlarr') return 'Prowlarr';
    if (name === 'sabnzbd') return 'SABnzbd';
    if (name === 'downloads') return 'Downloads folder';
    return name;
  }

  function setSignup(allow: boolean): void {
    if (!settings || allow === settings.allowSignup) return;
    const previous = settings;
    settings = { ...settings, allowSignup: allow };
    enqueue(async () => {
      if (settings?.allowSignup !== allow) return;
      try {
        const saved = await store.api.updateServerSettings({ allowSignup: allow });
        if (settings?.allowSignup === allow) settings = saved;
      } catch (error) {
        if (settings?.allowSignup === allow) settings = previous;
        store.notice = message(error);
      }
    });
  }

  function saveMb(): void {
    if (!settings) return;
    const shown = queuedText.maxEpubBytes ?? mbField(settings.maxEpubBytes);
    if (mb === shown) return;
    const bytes = parseMb(mb);
    if (bytes === null || (!queuedText.maxEpubBytes && bytes === settings.maxEpubBytes)) {
      mb = mbField(settings.maxEpubBytes);
      return;
    }
    const range = epubMbError(bytes);
    if (range) {
      mb = mbField(settings.maxEpubBytes);
      store.notice = range;
      return;
    }
    const typed = mb;
    queuedText.maxEpubBytes = typed;
    enqueue(async () => {
      try {
        settings = await store.api.updateServerSettings({ maxEpubBytes: bytes });
        if (mb === typed && settings) {
          mb = mbField(settings.maxEpubBytes);
          queuedText.maxEpubBytes = mb;
        }
      } catch (error) {
        if (mb === typed && settings) {
          mb = mbField(settings.maxEpubBytes);
          delete queuedText.maxEpubBytes;
        }
        store.notice = message(error);
      }
    });
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
        class:mine={user.mine}
        aria-pressed={user.isAdmin}
        aria-disabled={user.mine ? 'true' : undefined}
        aria-label={`${user.username} admin`}
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
  <h2 class="section-label">Book requests</h2>
  <label class="switch-row">
    <span>Requests</span>
    <input
      type="checkbox"
      role="switch"
      checked={settings.requestsEnabled === true}
      onchange={(event) => setRequests(event.currentTarget.checked)}
    />
  </label>
  <label class="settings-row text-row">
    <span>Prowlarr URL</span>
    <span class="field">
      <input
        class="text-field"
        value={prowlarrUrl}
        autocomplete="off"
        spellcheck="false"
        oninput={(event) => prowlarrUrl = event.currentTarget.value}
        onblur={() => saveText('prowlarrUrl', prowlarrUrl.trim())}
      />
    </span>
  </label>
  <label class="settings-row text-row">
    <span>Prowlarr API key</span>
    <span class="field">
      <input
        class="text-field"
        class:with-clear={settings.secrets?.prowlarrApiKey || prowlarrKey}
        type="password"
        value={prowlarrKey}
        placeholder={settings.secrets?.prowlarrApiKey && !prowlarrKey ? 'Saved' : ''}
        autocomplete="new-password"
        spellcheck="false"
        oninput={(event) => prowlarrKey = event.currentTarget.value}
        onblur={() => saveKey('prowlarrApiKey', prowlarrKey)}
      />
      {#if settings.secrets?.prowlarrApiKey || prowlarrKey}
        <button
          class="icon-button"
          type="button"
          aria-label="Clear Prowlarr API key"
          onmousedown={(event) => event.preventDefault()}
          onclick={() => clearKey('prowlarrApiKey')}
        >
          <Icon name="close" size={16} />
        </button>
      {/if}
    </span>
  </label>
  <label class="settings-row text-row">
    <span>SABnzbd URL</span>
    <span class="field">
      <input
        class="text-field"
        value={sabUrl}
        autocomplete="off"
        spellcheck="false"
        oninput={(event) => sabUrl = event.currentTarget.value}
        onblur={() => saveText('sabnzbdUrl', sabUrl.trim())}
      />
    </span>
  </label>
  <label class="settings-row text-row">
    <span>SABnzbd API key</span>
    <span class="field">
      <input
        class="text-field"
        class:with-clear={settings.secrets?.sabnzbdApiKey || sabKey}
        type="password"
        value={sabKey}
        placeholder={settings.secrets?.sabnzbdApiKey && !sabKey ? 'Saved' : ''}
        autocomplete="new-password"
        spellcheck="false"
        oninput={(event) => sabKey = event.currentTarget.value}
        onblur={() => saveKey('sabnzbdApiKey', sabKey)}
      />
      {#if settings.secrets?.sabnzbdApiKey || sabKey}
        <button
          class="icon-button"
          type="button"
          aria-label="Clear SABnzbd API key"
          onmousedown={(event) => event.preventDefault()}
          onclick={() => clearKey('sabnzbdApiKey')}
        >
          <Icon name="close" size={16} />
        </button>
      {/if}
    </span>
  </label>
  <label class="settings-row text-row">
    <span>SABnzbd category</span>
    <span class="field">
      <input
        class="text-field"
        value={category}
        autocomplete="off"
        spellcheck="false"
        oninput={(event) => category = event.currentTarget.value}
        onblur={() => saveText('sabnzbdCategory', category.trim())}
      />
    </span>
  </label>
  <div class="fact-row">
    <span>Downloads folder</span>
    <span>{settings.downloadsDir ?? 'Not set'}</span>
  </div>
  <button class="settings-row" disabled={testing} aria-busy={testing} onclick={() => testConnection()}>
    <span class="accent">{testing ? 'Testing…' : 'Test connection'}</span>
    {#if testing}
      <span class="spinner" aria-hidden="true"></span>
    {:else}
      <Icon name="chevron" />
    {/if}
  </button>
  {#if checks}
    {#each checks as check (check.name)}
      <div class="fact-row">
        <span>{checkName(check.name)}</span>
        <span class:error={!check.ok}>{check.detail}</span>
      </div>
    {/each}
  {/if}
  <button class="settings-row" onclick={() => router.go('/settings/requests?all=1')}>
    <span>Requests</span>
    <span class="row-trail">
      {#if requestCount != null}<span class="count">{requestCount}</span>{/if}
      <Icon name="chevron" />
    </span>
  </button>
  <button class="settings-row" onclick={() => router.go('/settings/server/users')}>
    <span>Users</span>
    <span class="row-trail">
      <span class="count">{info.users}</span>
      <Icon name="chevron" />
    </span>
  </button>
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

.row-trail {
  display: inline-flex;
  align-items: center;
  gap: 12px;
}

.count {
  font-weight: 400;
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

.text-row > span:first-child {
  flex: 1 1 auto;
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.field {
  position: relative;
  flex: 0 0 auto;
  width: min(16rem, 50%);
  max-width: 16rem;
}

.text-field {
  width: 100%;
  min-width: 0;
  height: 36px;
  padding: 0 8px;
  color: var(--ink);
  background: var(--ground);
  border: 1px solid var(--line);
  border-radius: 4px;
  font: inherit;
  font-weight: 400;
}

.text-field.with-clear {
  padding-right: 32px;
}

.field .icon-button {
  position: absolute;
  top: 2px;
  right: 0;
  width: 32px;
  height: 32px;
  color: var(--mute);
}

.accent {
  color: var(--accent);
}

.settings-row:disabled {
  opacity: 1;
}

.settings-row .spinner {
  width: 18px;
  height: 18px;
  flex: 0 0 18px;
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

.shield.mine {
  cursor: default;
  pointer-events: none;
}

.shield[aria-pressed='true'] {
  color: var(--accent);
}

.shield:not([aria-pressed='true']) {
  color: var(--mute);
}
</style>
