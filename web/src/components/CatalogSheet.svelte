<script lang="ts">
  import { untrack } from 'svelte';
  import { ApiError } from '../lib/api';
  import type { BookRequest, CatalogWork, RequestDetail, WorkDetail } from '../lib/models';
  import {
    alsoRequested, catalogErrorText, catalogOlid, copyLabel, coverUrl, heldCopy, isLivePhase,
    noMatchCopy, requestPhase, subjectLine, waitingCopy, workStats,
  } from '../lib/requests';
  import { readerUrl, router } from '../lib/router.svelte';
  import { message, store } from '../lib/store.svelte';
  import BookCover from './BookCover.svelte';
  import Icon from './Icon.svelte';
  import Sheet from './Sheet.svelte';

  let { work, request, onchange, onclose }: {
    work: CatalogWork;
    request: BookRequest | null;
    onchange: (request: BookRequest | null) => void;
    onclose: () => void;
  } = $props();

  let detail = $state<WorkDetail | null>(null);
  let detailError = $state('');
  let actionError = $state('');
  let loading = $state(true);
  let acting = $state(false);
  let attempt = $state(0);
  let expanded = $state(false);
  let overflow = $state(false);
  let importedShelf = false;

  let info = $derived(detail?.info ?? null);
  let liveRequest = $derived<RequestDetail | BookRequest | null>(detail?.status.request ?? request);
  let copy = $derived(detail?.status.onServer ?? work.onServer ?? null);
  let phase = $derived(liveRequest ? requestPhase(liveRequest) : null);
  let title = $derived(work.title);
  let author = $derived(info?.authors.length ? info.authors.join(', ') : work.author);
  let year = $derived(info?.year ?? work.year);
  let coverId = $derived(info?.coverId ?? work.coverId);
  let atCap = $derived(detail != null && detail.status.active >= detail.status.cap);
  let holders = $derived(detail?.status.otherRequests.map((row) => row.requester) ?? []);
  let bookId = $derived(
    detail?.status.request?.bookId
      ?? detail?.status.onServer?.bookId
      ?? request?.bookId
      ?? work.onServer?.bookId
      ?? null,
  );

  function sameRequest(left: BookRequest | null, right: BookRequest | null): boolean {
    if (left === right) return true;
    if (!left || !right) return false;
    const leftPhase = 'phase' in left ? left.phase : undefined;
    const rightPhase = 'phase' in right ? right.phase : undefined;
    return left.id === right.id
      && left.state === right.state
      && left.progress === right.progress
      && left.error === right.error
      && left.bookId === right.bookId
      && leftPhase === rightPhase;
  }

  function apply(next: WorkDetail): void {
    const previous = detail?.status.request ?? null;
    const opening = detail == null;
    detail = next;
    detailError = '';
    loading = false;
    if (!sameRequest(previous, next.status.request)) onchange(next.status.request);
    if (next.status.request?.phase !== 'imported') return;
    // The shelf already has a book that was imported before the sheet opened.
    if (opening) {
      importedShelf = true;
      return;
    }
    if (!importedShelf) {
      importedShelf = true;
      void store.load();
    }
  }

  $effect(() => {
    const olid = catalogOlid(work.workKey);
    const generation = attempt;
    let alive = true;
    let timer: ReturnType<typeof setTimeout> | undefined;
    const ctrl = new AbortController();

    async function pull(): Promise<void> {
      try {
        const next = await store.api.catalogWork(olid, ctrl.signal);
        if (!alive) return;
        untrack(() => apply(next));
        if (!alive) return;
        if (isLivePhase(next.status.request?.phase)) {
          timer = setTimeout(() => { void pull(); }, 10_000);
        }
      } catch (error) {
        if (!alive || ctrl.signal.aborted) return;
        if (error instanceof DOMException && error.name === 'AbortError') return;
        if (detail == null) {
          const status = error instanceof ApiError ? error.status : -1;
          detailError = catalogErrorText(status, message(error));
          loading = false;
        }
        timer = setTimeout(() => { void pull(); }, 10_000);
      }
    }

    void generation;
    void pull();
    return () => {
      alive = false;
      clearTimeout(timer);
      ctrl.abort();
    };
  });

  function measure(node: HTMLElement): { destroy: () => void } {
    const update = (): void => {
      overflow = node.scrollHeight > node.clientHeight + 1;
    };
    update();
    const frame = requestAnimationFrame(update);
    return { destroy: () => cancelAnimationFrame(frame) };
  }

  async function requestBook(): Promise<void> {
    if (acting || atCap) return;
    acting = true;
    actionError = '';
    try {
      const next = await store.api.requestBook(work);
      onchange(next);
      if (next.state === 'imported') {
        importedShelf = true;
        void store.load();
      }
      attempt += 1;
    } catch (error) {
      actionError = message(error);
    } finally {
      acting = false;
    }
  }

  async function cancel(): Promise<void> {
    const current = liveRequest;
    if (acting || !current) return;
    acting = true;
    actionError = '';
    try {
      await store.api.cancelRequest(current.id);
      const next = { ...current, state: 'canceled' as const, error: null, phase: 'canceled' as const };
      if (detail?.status.request?.id === current.id) {
        detail = {
          ...detail,
          status: {
            ...detail.status,
            request: { ...detail.status.request, state: 'canceled', error: null, phase: 'canceled' },
          },
        };
      }
      onchange(next);
      attempt += 1;
    } catch (error) {
      actionError = message(error);
    } finally {
      acting = false;
    }
  }

  async function retry(): Promise<void> {
    const current = liveRequest;
    if (acting || atCap || !current) return;
    acting = true;
    actionError = '';
    try {
      const next = await store.api.retryRequest(current.id);
      if (detail?.status.request) {
        detail = {
          ...detail,
          status: {
            ...detail.status,
            request: { ...detail.status.request, ...next, phase: requestPhase(next) },
          },
        };
      }
      onchange(next);
      if (next.state === 'imported') {
        importedShelf = true;
        void store.load();
      }
      attempt += 1;
    } catch (error) {
      actionError = message(error);
    } finally {
      acting = false;
    }
  }

  function openBook(): void {
    if (!bookId) return;
    onclose();
    router.go(readerUrl(bookId));
  }

  let detailRequest = $derived(
    detail?.status.request && liveRequest && detail.status.request.id === liveRequest.id
      ? detail.status.request
      : null,
  );
</script>

<Sheet label={title} kind="full" {onclose}>
  <div class="sheet-body">
    <header class="sheet-bar">
      <button type="button" class="icon-button" aria-label="Close" onclick={onclose}>
        <Icon name="close" />
      </button>
    </header>
    <div class="sheet-scroll">
      <div class="hero">
        <BookCover {title} src={coverUrl(coverId)} width={72} height={108} />
        <div class="post-column">
          <h3>{title}</h3>
          <p class="meta">{author}{year != null ? ` · ${year}` : ''}</p>
        </div>
      </div>

      <section class="status" aria-label="Download status">
        {#if phase && isLivePhase(phase)}
          {#if phase === 'searching'}
            <p>Looking for a release…</p>
          {:else if phase === 'waiting'}
            <p>{waitingCopy(detailRequest?.nextCheckAt)}</p>
          {:else if phase === 'no_match'}
            <p>{noMatchCopy(liveRequest?.error, detailRequest?.nextCheckAt)}</p>
          {:else if phase === 'held'}
            <p>{heldCopy(liveRequest?.error)}</p>
          {:else if phase === 'downloading'}
            <p>Downloading {liveRequest?.progress ?? 0}%</p>
            <progress
              max="100"
              value={liveRequest?.progress ?? 0}
              aria-label={`Downloading ${liveRequest?.progress ?? 0}%`}
            ></progress>
          {:else if phase === 'retrying'}
            <p>{liveRequest?.error || 'Trying another release…'}</p>
          {:else}
            <p>Checking and importing…</p>
            <progress max="100" value={liveRequest?.progress ?? 0} aria-label="Importing"></progress>
          {/if}
          <button class="pill action secondary" type="button" disabled={acting} onclick={() => { void cancel(); }}>
            Cancel request
          </button>
        {:else if phase === 'failed' && liveRequest}
          <p>{liveRequest.error ?? 'The download failed.'}</p>
          <button class="pill action" type="button" disabled={acting || atCap} onclick={() => { void retry(); }}>
            Retry
          </button>
          {#if atCap}<p class="muted">You have {detail?.status.cap} requests in progress</p>{/if}
        {:else if copy?.bookId || (phase === 'imported' && liveRequest?.bookId)}
          <p>{copy ? copyLabel(copy, 'sheet') : 'On your shelf'}</p>
          <button class="pill action" type="button" disabled={!bookId} onclick={openBook}>Open book</button>
        {:else if phase === 'canceled'}
          <button class="pill action" type="button" disabled={acting || atCap} onclick={() => { void retry(); }}>
            Request again
          </button>
          {#if atCap}<p class="muted">You have {detail?.status.cap} requests in progress</p>{/if}
        {:else if phase === 'imported'}
          <button class="pill action" type="button" disabled={acting || atCap} onclick={() => { void requestBook(); }}>
            Request again
          </button>
          {#if atCap}<p class="muted">You have {detail?.status.cap} requests in progress</p>{/if}
        {:else}
          <button class="pill action" type="button" disabled={acting || atCap} onclick={() => { void requestBook(); }}>
            Request
          </button>
          {#if atCap}<p class="muted">You have {detail?.status.cap} requests in progress</p>{/if}
          {#if info && !info.hasAcceptedEdition}
            <p class="muted">No English edition listed on Open Library</p>
          {/if}
        {/if}
        {#if holders.length > 0}
          <p class="muted">{alsoRequested(holders)}</p>
        {/if}
        {#if actionError}
          <p class="action-error" role="alert">{actionError}</p>
        {/if}
      </section>

      {#if loading && !info}
        <div class="bones" aria-busy="true">
          <p class="sr-only" role="status">Loading details…</p>
          <span class="bone"></span>
          <span class="bone mid"></span>
          <span class="bone"></span>
        </div>
      {:else if detailError}
        <div class="detail-error">
          <p class="muted">{detailError}</p>
          <button type="button" class="text-button" onclick={() => attempt += 1}>Try again</button>
        </div>
      {:else if detail && !info}
        <div class="detail-error">
          <p class="muted">Couldn't load details from Open Library.</p>
          <button type="button" class="text-button" onclick={() => attempt += 1}>Try again</button>
        </div>
      {:else if info}
        <div class="about">
          {#if info.description}
            <div class="blurb">
              <p class="description" class:clamped={!expanded} use:measure>{info.description}</p>
              {#if overflow || expanded}
                <button type="button" class="text-button" onclick={() => expanded = !expanded}>
                  {expanded ? 'Less' : 'More'}
                </button>
              {/if}
            </div>
          {/if}
          {#if info.series.length || info.awards.length}
            <div class="rows">
              {#if info.series.length}
                <div class="row">
                  <span>Series</span>
                  <span>{info.series.join(' · ')}</span>
                </div>
              {/if}
              {#if info.awards.length}
                <div class="row">
                  <span>Awards</span>
                  <span>{info.awards.join(' · ')}</span>
                </div>
              {/if}
            </div>
          {/if}
          {#if info.subjects.length}
            <p class="subjects">{subjectLine(info.subjects)}</p>
          {/if}
          {#if workStats(info)}<p class="stats">{workStats(info)}</p>{/if}
          <a
            class="library"
            href="https://openlibrary.org{info.canonicalKey}"
            target="_blank"
            rel="noopener"
          >
            View on Open Library
            <Icon name="external" size={16} />
          </a>
        </div>
      {/if}
    </div>
  </div>
</Sheet>

<style>
  .sheet-body {
    height: 100%;
    display: flex;
    flex-direction: column;
  }

  .sheet-scroll {
    overflow-y: auto;
    min-height: 0;
    flex: 1;
    padding-bottom: 24px;
  }

  .hero {
    display: flex;
    gap: 16px;
    padding: 16px 16px 0;
  }

  h3 {
    margin: 0;
    font-size: 1.25rem;
    line-height: 1.25;
    font-weight: 700;
  }

  .hero .meta {
    margin-top: 4px;
    font-size: .9375rem;
    line-height: 20px;
  }

  .status {
    display: flex;
    flex-direction: column;
    align-items: stretch;
    gap: 12px;
    margin: 24px 16px 0;
  }

  .status > p:not(.muted):not(.action-error) {
    font-size: 1.0625rem;
    line-height: 24px;
  }

  .action-error {
    margin: 0;
    color: var(--like);
    font-size: .9375rem;
    line-height: 20px;
  }

  .action {
    width: 100%;
    box-sizing: border-box;
    min-height: 44px;
    font-size: .9375rem;
  }

  .action.secondary {
    background: transparent;
    color: var(--like);
    border: 1px solid var(--line);
  }

  .action.secondary:hover:not(:disabled) {
    background: color-mix(in srgb, var(--like) 8%, transparent);
  }

  progress {
    width: 100%;
    height: 4px;
    border: 0;
    border-radius: 999px;
    background: var(--line);
    accent-color: var(--accent);
  }

  progress::-webkit-progress-bar { background: var(--line); }
  progress::-webkit-progress-value { background: var(--accent); }
  progress::-moz-progress-bar { background: var(--accent); }

  .about {
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: 24px;
    margin-top: 28px;
    padding: 0 16px 32px;
  }

  .blurb {
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: 4px;
  }

  .description {
    font-size: 1.0625rem;
    line-height: 24px;
    white-space: pre-wrap;
    overflow-wrap: anywhere;
  }

  .clamped {
    display: -webkit-box;
    -webkit-line-clamp: 6;
    line-clamp: 6;
    -webkit-box-orient: vertical;
    overflow: hidden;
  }

  .rows {
    display: flex;
    flex-direction: column;
    gap: 8px;
    width: 100%;
  }

  .row {
    display: grid;
    grid-template-columns: 4.75rem 1fr;
    column-gap: 16px;
    font-size: 1.0625rem;
    line-height: 24px;
  }

  .row span:first-child {
    color: var(--mute);
  }

  .subjects {
    color: var(--mute);
    font-size: .9375rem;
    line-height: 20px;
  }

  .stats {
    font-size: .9375rem;
    line-height: 20px;
  }

  .library {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    min-height: 40px;
    padding: 0;
    color: var(--accent);
    font-size: .9375rem;
    font-weight: 700;
  }

  .text-button {
    padding-left: 0;
    padding-right: 0;
    font-size: .9375rem;
    font-weight: 700;
  }

  .detail-error {
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    margin-top: 28px;
    padding: 0 16px 16px;
  }

  .detail-error p {
    font-size: .9375rem;
    line-height: 20px;
  }

  .bones {
    display: flex;
    flex-direction: column;
    align-items: stretch;
    gap: 10px;
    margin-top: 28px;
    padding: 0 16px;
  }

  .bone {
    display: block;
    height: 14px;
    width: 100%;
    border-radius: 4px;
    background: color-mix(in srgb, var(--mute) 16%, var(--ground));
  }

  .bone.mid {
    width: 64%;
  }
</style>
