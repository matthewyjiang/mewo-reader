<script lang="ts">
  import type { BookRequest, CatalogWork } from '../lib/models';
  import { badge, badgeTone, catalogRowLabel, coverUrl, openLabel, rowAction } from '../lib/requests';
  import BookCover from './BookCover.svelte';
  import Icon from './Icon.svelte';

  let { work, request, onopen, onact }: {
    work: CatalogWork;
    request: BookRequest | null;
    onopen: () => void;
    onact: () => void;
  } = $props();

  let action = $derived(rowAction(request, work.onServer));
  let label = $derived(badge(work, request));
  let tone = $derived(label ? badgeTone(label) : 'progress');
  let actionName = $derived(
    action === 'again' ? 'Request again'
      : action === 'retry' ? 'Retry'
        : action === 'open' ? openLabel(work, request)
          : 'Request',
  );
  let icon = $derived<'plus' | 'book' | 'retry'>(
    action === 'open' ? 'book' : action === 'retry' ? 'retry' : 'plus',
  );
</script>

<div class="book-row" class:live={action === 'pending'}>
  <button type="button" class="book-link" aria-label={catalogRowLabel(work, request)} onclick={onopen}>
    <BookCover title={work.title} src={coverUrl(work.coverId)} />
    <span class="post-column">
      <strong class="title truncate">{work.title}</strong>
      <span class="author">{work.author}{work.year != null ? ` · ${work.year}` : ''}</span>
      {#if label}
        <span class="badge">
          <span class="dot {tone}"></span>
          {label}
        </span>
      {/if}
    </span>
  </button>
  {#if action !== 'pending'}
    <button type="button" class="row-action" aria-label={actionName} onclick={onact}>
      <span class="circle">
        <Icon name={icon} size={16} />
      </span>
    </button>
  {/if}
</div>

<style>
  .book-row {
    display: flex;
    align-items: center;
    border-bottom: .6px solid var(--line);
    padding: 12px 4px 12px 0;
  }

  .book-row.live {
    padding-right: 0;
  }

  .book-row:hover {
    background: var(--hover);
  }

  .book-link {
    flex: 1;
    min-width: 0;
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 0 0 0 16px;
    text-align: left;
    font: inherit;
    color: inherit;
  }

  .book-row.live .book-link {
    padding-right: 16px;
  }

  .book-link .post-column {
    display: flex;
    flex-direction: column;
    gap: 2px;
  }

  .title {
    font-size: 1.0625rem;
    line-height: 22px;
    font-weight: 700;
  }

  .author {
    color: var(--mute);
    font-size: .9375rem;
    line-height: 20px;
  }

  .badge {
    display: flex;
    align-items: center;
    gap: 6px;
    color: var(--ink);
    font-size: .8125rem;
    line-height: 16px;
  }

  .dot {
    width: 8px;
    height: 8px;
    flex: 0 0 8px;
    border-radius: 50%;
    background: var(--accent);
  }

  .dot.shelf {
    background: var(--green);
  }

  .dot.failed {
    background: var(--like);
  }

  .dot.server {
    background: var(--accent);
  }

  .dot.progress {
    background: var(--mute);
  }

  .row-action {
    display: flex;
    align-items: center;
    justify-content: center;
    flex: 0 0 44px;
    width: 44px;
    height: 44px;
    padding: 0;
    color: var(--accent);
  }

  .circle {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 32px;
    height: 32px;
    border: 1.5px solid var(--accent);
    border-radius: 50%;
  }
</style>
