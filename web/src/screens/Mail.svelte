<script lang="ts">
  import { onMount } from 'svelte';
  import Icon from '../components/Icon.svelte';
  import { ApiError } from '../lib/api';
  import type { ServerSettings, ServiceCheck, SettingsPatch, SmtpTls } from '../lib/models';
  import { restoreScroll } from '../lib/screen';
  import { message, store } from '../lib/store.svelte';

  let { scroller }: { scroller: HTMLElement } = $props();

  type TextKey = 'publicUrl' | 'smtpHost' | 'smtpUsername' | 'mailFrom';

  let status = $state<'loading' | 'ready' | 'denied'>('loading');
  let settings = $state<ServerSettings | null>(null);
  let text = $state<Record<TextKey, string>>({
    publicUrl: '', smtpHost: '', smtpUsername: '', mailFrom: '',
  });
  let port = $state('587');
  let password = $state('');
  let to = $state('');
  let testing = $state(false);
  let result = $state<ServiceCheck | null>(null);
  let pending: Promise<void> = Promise.resolve();

  const securities: { value: SmtpTls; label: string }[] = [
    { value: 'starttls', label: 'STARTTLS' },
    { value: 'tls', label: 'TLS' },
    { value: 'none', label: 'None' },
  ];

  function show(next: ServerSettings): void {
    settings = next;
    text = {
      publicUrl: next.publicUrl ?? '',
      smtpHost: next.smtpHost ?? '',
      smtpUsername: next.smtpUsername ?? '',
      mailFrom: next.mailFrom ?? '',
    };
    port = String(next.smtpPort ?? 587);
  }

  onMount(() => {
    restoreScroll(scroller);
    void store.api.serverSettings()
      .then((next) => {
        show(next);
        status = 'ready';
      })
      .catch((error: unknown) => {
        status = 'denied';
        if (!(error instanceof ApiError && error.status === 403)) store.notice = message(error);
      });
  });

  // One save at a time, in the order they were made. Only the saved keys are
  // copied back, so a draft in another field survives. A failed save shows the
  // stored value again.
  function save(patch: SettingsPatch): void {
    const keys = Object.keys(patch);
    pending = pending.then(async () => {
      try {
        const saved = await store.api.updateServerSettings(patch);
        settings = saved;
        sync(saved, keys);
        void store.loadMe();
      } catch (error) {
        if (settings) sync(settings, keys);
        store.notice = message(error);
      }
    });
  }

  function sync(from: ServerSettings, keys: string[]): void {
    for (const key of keys) {
      if (key === 'smtpPort') port = String(from.smtpPort);
      else if (key in text) text[key as TextKey] = from[key as TextKey] ?? '';
    }
  }

  function saveText(key: TextKey): void {
    const value = text[key].trim();
    if (!settings || value === (settings[key] ?? '')) return;
    save({ [key]: value });
  }

  function savePort(): void {
    if (!settings) return;
    const value = Number(port.trim());
    if (value === settings.smtpPort) return;
    if (!Number.isInteger(value) || value < 1 || value > 65535) {
      port = String(settings.smtpPort);
      store.notice = 'Port must be 1 to 65535.';
      return;
    }
    save({ smtpPort: value });
  }

  function saveSecurity(value: SmtpTls): void {
    if (!settings || value === settings.smtpTls) return;
    save({ smtpTls: value });
  }

  function savePassword(): void {
    const value = password.trim();
    if (!value) return;
    password = '';
    save({ smtpPassword: value });
  }

  function clearPassword(): void {
    if (password) {
      password = '';
      return;
    }
    if (settings?.secrets?.smtpPassword) save({ smtpPassword: null });
  }

  async function sendTest(event: SubmitEvent): Promise<void> {
    event.preventDefault();
    if (testing || !to.trim()) return;
    testing = true;
    result = null;
    try {
      await pending;
      result = await store.api.testMail(to);
    } catch (error) {
      store.notice = message(error);
    } finally {
      testing = false;
    }
  }
</script>

{#snippet textRow(key: TextKey, label: string, placeholder = '', type = 'text')}
  <label class="settings-row text-row">
    <span>{label}</span>
    <span class="field">
      <input
        class="text-field"
        {type}
        value={text[key]}
        {placeholder}
        autocomplete="off"
        autocapitalize="none"
        spellcheck="false"
        oninput={(event) => text[key] = event.currentTarget.value}
        onblur={() => saveText(key)}
      />
    </span>
  </label>
{/snippet}

{#if status === 'loading'}
  <p class="empty-state">Loading…</p>
{:else if status === 'denied' || !settings}
  <p class="empty-state">Admins only.</p>
{:else}
  {@render textRow('publicUrl', 'Public URL', location.origin, 'url')}
  {@render textRow('smtpHost', 'SMTP host', 'smtp.example.com')}
  <label class="settings-row text-row">
    <span>Port</span>
    <span class="field">
      <input
        class="text-field"
        value={port}
        inputmode="numeric"
        autocomplete="off"
        oninput={(event) => port = event.currentTarget.value}
        onblur={savePort}
      />
    </span>
  </label>
  <label class="settings-row text-row">
    <span>Security</span>
    <span class="field">
      <select
        class="text-field"
        value={settings.smtpTls}
        onchange={(event) => saveSecurity(event.currentTarget.value as SmtpTls)}
      >
        {#each securities as option (option.value)}
          <option value={option.value}>{option.label}</option>
        {/each}
      </select>
    </span>
  </label>
  {@render textRow('smtpUsername', 'Username')}
  <label class="settings-row text-row">
    <span>Password</span>
    <span class="field">
      <input
        class="text-field"
        class:with-clear={settings.secrets?.smtpPassword || password}
        type="password"
        value={password}
        placeholder={settings.secrets?.smtpPassword && !password ? 'Saved' : ''}
        autocomplete="new-password"
        spellcheck="false"
        oninput={(event) => password = event.currentTarget.value}
        onblur={savePassword}
      />
      {#if settings.secrets?.smtpPassword || password}
        <button
          class="icon-button"
          type="button"
          aria-label="Clear SMTP password"
          onmousedown={(event) => event.preventDefault()}
          onclick={clearPassword}
        >
          <Icon name="close" size={16} />
        </button>
      {/if}
    </span>
  </label>
  {@render textRow('mailFrom', 'From', 'Mewo <mewo@example.com>')}
  <form class="settings-row text-row" onsubmit={sendTest}>
    <label for="mail-test-to">Send test to</label>
    <span class="test-field">
      <input
        id="mail-test-to"
        class="text-field"
        type="email"
        value={to}
        autocomplete="email"
        spellcheck="false"
        oninput={(event) => to = event.currentTarget.value}
      />
      <button type="submit" class="pill send" disabled={testing || !to.trim()} aria-busy={testing}>
        {testing ? 'Sending…' : 'Send'}
      </button>
    </span>
  </form>
  {#if result}
    <div class="fact-row">
      <span>Test</span>
      <span class:error={!result.ok}>{result.detail}</span>
    </div>
  {/if}
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

.text-row > :first-child {
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

.test-field {
  flex: 0 0 auto;
  width: min(16rem, 60%);
  display: flex;
  align-items: center;
  gap: 8px;
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

.send {
  flex: 0 0 auto;
  min-height: 36px;
  padding: 6px 14px;
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
