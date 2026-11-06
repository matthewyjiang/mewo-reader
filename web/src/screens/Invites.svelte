<script lang="ts">
  import { onMount } from 'svelte';
  import { ApiError } from '../lib/api';
  import { DEFAULT_INVITE_DAYS, inviteLink, inviteTrail } from '../lib/invites';
  import type { AdminInvite } from '../lib/models';
  import { router } from '../lib/router.svelte';
  import { restoreScroll } from '../lib/screen';
  import { copyText } from '../lib/share';
  import { message, store } from '../lib/store.svelte';

  let { scroller }: { scroller: HTMLElement } = $props();

  let status = $state<'loading' | 'ready' | 'denied'>('loading');
  let items = $state<AdminInvite[]>([]);
  let note = $state('');
  let days = $state<number | null>(DEFAULT_INVITE_DAYS);
  let link = $state<string | null>(null);
  let copied = $state(false);
  let creating = $state(false);
  let copyTimer: ReturnType<typeof setTimeout> | undefined;
  let generation = 0;

  onMount(() => {
    restoreScroll(scroller);
    const ticket = ++generation;
    void load(ticket, true);
    return () => {
      generation += 1;
      clearTimeout(copyTimer);
    };
  });

  async function load(ticket: number, initial: boolean): Promise<void> {
    try {
      const next = await store.api.adminInvites();
      if (ticket !== generation) return;
      items = next;
      status = 'ready';
    } catch (error) {
      if (ticket !== generation) return;
      if (initial) {
        status = 'denied';
        if (!(error instanceof ApiError && error.status === 403)) store.notice = message(error);
        return;
      }
      store.notice = message(error);
    }
  }

  function lifetime(): number | null {
    if (days === null || !Number.isInteger(days) || days < 1 || days > 90) return null;
    return days;
  }

  async function create(event: SubmitEvent): Promise<void> {
    event.preventDefault();
    if (creating) return;
    const span = lifetime();
    if (span === null) {
      store.notice = 'Expiry must be 1 to 90 days.';
      return;
    }
    creating = true;
    try {
      const created = await store.api.createInvite(note, span);
      link = inviteLink(location.origin, created.token);
      copied = false;
      note = '';
      days = DEFAULT_INVITE_DAYS;
      items = [created.invite, ...items.filter((row) => row.id !== created.invite.id)];
    } catch (error) {
      store.notice = message(error);
    } finally {
      creating = false;
    }
  }

  async function copy(): Promise<void> {
    if (!link) return;
    try {
      await copyText(link, 'Select the link and copy it manually.');
      copied = true;
      clearTimeout(copyTimer);
      copyTimer = setTimeout(() => {
        copied = false;
      }, 2000);
    } catch {
      copied = false;
    }
  }

  async function revoke(invite: AdminInvite): Promise<void> {
    try {
      const saved = await store.api.revokeInvite(invite.id);
      items = items.map((row) => row.id === saved.id ? saved : row);
    } catch (error) {
      store.notice = message(error);
      if (error instanceof ApiError && error.status === 409) await load(generation, false);
    }
  }
</script>

{#if status === 'denied'}
  <div class="empty-state">
    <h2>Not found</h2>
    <button class="pill" onclick={() => router.go('/settings')}>Back to Settings</button>
  </div>
{:else if status === 'ready'}
  <form class="create" onsubmit={create}>
    <label class="settings-row">
      <span>Note</span>
      <input class="text-field" maxlength="200" bind:value={note} />
    </label>
    <label class="settings-row">
      <span>Expires in days</span>
      <input
        class="text-field days"
        type="number"
        min="1"
        max="90"
        step="1"
        inputmode="numeric"
        bind:value={days}
      />
    </label>
    <div class="actions">
      <button type="submit" class="pill" disabled={creating}>
        {creating ? 'Creating…' : 'Create invite'}
      </button>
    </div>
  </form>
  {#if link}
    <div class="link-block">
      <label class="link-field">
        <span class="sr-only">Invite link</span>
        <input class="text-field" readonly value={link} />
      </label>
      <button type="button" class="pill" onclick={() => void copy()}>
        {copied ? 'Copied' : 'Copy link'}
      </button>
    </div>
  {/if}
  {#each items as invite (invite.id)}
    <div class="settings-row">
      <span class="name">{invite.note ?? 'Invite'}</span>
      <span class="row-trail">
        <span class="trail">{inviteTrail(invite, Date.now())}</span>
        {#if invite.state === 'active'}
          <button type="button" class="pill revoke" onclick={() => void revoke(invite)}>Revoke</button>
        {/if}
      </span>
    </div>
  {/each}
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

  .text-field {
    width: min(16rem, 52%);
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

  .days {
    width: 5.5rem;
  }

  .actions, .link-block {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 12px;
    padding: 16px;
    border-bottom: .6px solid var(--line);
  }

  .link-field {
    flex: 1 1 12rem;
    min-width: 0;
  }

  .link-field .text-field {
    width: 100%;
  }

  .name {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .row-trail {
    display: inline-flex;
    align-items: center;
    gap: 12px;
    flex: 0 0 auto;
  }

  .trail {
    font-weight: 400;
  }

  .revoke {
    background: transparent;
    color: var(--like);
    border: 1px solid var(--like);
    padding: 6px 14px;
  }

  .revoke:hover:not(:disabled) {
    background: color-mix(in srgb, var(--like) 12%, transparent);
  }
</style>
