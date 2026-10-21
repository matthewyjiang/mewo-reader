<script lang="ts">
  import type { BookRequest, CatalogWork } from '../lib/models';
  import { badge, badgeTone, catalogRowLabel, coverUrl, openLabel, rowAction, stateLabel } from '../lib/requests';
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
    action === 'request' ? 'Request'
      : action === 'pending' && request ? stateLabel(request)
        : action === 'open' ? openLabel(work, request)
          : 'Retry',
  );
</script>

<div class="book-row">
  <button type="button" class="book-link" aria-label={catalogRowLabel(work, request)} onclick={onopen}>
    <BookCover title={work.title} src={coverUrl(work.coverId)} />
    <span class="post-column">
      <span class="post-meta">
        <strong class="truncate">{work.title}</strong>
      </span>
      <span class="meta author">{work.author}{work.year != null ? ` · ${work.year}` : ''}</span>
      {#if label}
        <span class="meta badge">
          <span class="dot {tone}"></span>
          {label}
        </span>
      {/if}
    </span>
  </button>
  <div class="book-actions">
    <button
      type="button"
      class="icon-button"
      aria-label={actionName}
      disabled={action === 'pending'}
      onclick={onact}
    >
      <Icon
        name={action === 'request' ? 'plus' : action === 'pending' ? 'clock' : action === 'open' ? 'check' : 'retry'}
        size={18}
      />
    </button>
  </div>
</div>

<style>
  .book-row {
    display: flex;
    align-items: flex-start;
    border-bottom: .6px solid var(--line);
    padding-right: 12px;
  }

  .book-row:hover {
    background: var(--hover);
  }

  .book-link {
    flex: 1;
    min-width: 0;
    display: flex;
    gap: 12px;
    padding: 12px 4px 16px 16px;
    text-align: left;
    font: inherit;
    color: inherit;
  }

  .author, .badge {
    display: block;
    margin-top: 3px;
    font-size: .9375rem;
    line-height: 1.33;
  }

  .badge {
    display: flex;
    align-items: center;
    gap: 6px;
    font-size: .8125rem;
    line-height: 1.23;
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

  .book-row .post-meta {
    height: 21px;
  }

  .book-row .post-meta strong {
    font-size: 1.0625rem;
    line-height: 21px;
  }

  .book-actions {
    display: flex;
    align-items: center;
    flex: 0 0 auto;
    margin-top: 12px;
    height: 21px;
    min-height: 0;
  }

  .book-actions > .icon-button {
    width: 40px;
    height: 40px;
    padding: 0;
    line-height: 0;
    color: var(--mute);
  }
</style>
