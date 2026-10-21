<script lang="ts">
  import { untrack } from 'svelte';
  import type { BookRequest, CatalogWork, RequestDetail, WorkDetail, WorkInfo } from '../lib/models';
  import { relativeTime } from '../lib/models';
  import {
    badge, badgeTone, catalogOlid, copyLabel, coverUrl, heldCopy, isLivePhase,
    noMatchCopy, requestPhase, stateLabel, waitingCopy,
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
  let shownWork = $derived({ ...work, onServer: copy });
  let badgeLabel = $derived(badge(shownWork, liveRequest));
  let tone = $derived(badgeLabel ? badgeTone(badgeLabel) : 'progress');
  let atCap = $derived(detail != null && detail.status.active >= detail.status.cap);
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
          detailError = message(error);
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

  function facts(value: WorkInfo): string {
    const parts: string[] = [];
    if (value.year != null) parts.push(`First published ${value.year}`);
    if (value.editionCount != null) parts.push(`${value.editionCount.toLocaleString()} editions`);
    if (value.pagesMedian != null) parts.push(`~${value.pagesMedian.toLocaleString()} pages`);
    if (value.rating) {
      const average = value.rating.average.toFixed(1);
      parts.push(`★ ${average} (${value.rating.count.toLocaleString()})`);
    }
    return parts.join(' · ');
  }

  function editionLine(languages: string[]): string {
    if (languages.length === 0) return '';
    const shown = languages.slice(0, 6);
    const extra = languages.length - shown.length;
    const list = shown.join(', ');
    return extra > 0 ? `Editions in ${list} +${extra}` : `Editions in ${list}`;
  }

  async function requestBook(): Promise<void> {
    if (acting || atCap) return;
    acting = true;
    try {
      const next = await store.api.requestBook(work);
      onchange(next);
      if (next.state === 'imported') {
        importedShelf = true;
        void store.load();
      }
      attempt += 1;
    } catch (error) {
      store.notice = message(error);
    } finally {
      acting = false;
    }
  }

  async function cancel(): Promise<void> {
    const current = liveRequest;
    if (acting || !current) return;
    acting = true;
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
      store.notice = message(error);
    } finally {
      acting = false;
    }
  }

  async function retry(): Promise<void> {
    const current = liveRequest;
    if (acting || atCap || !current) return;
    acting = true;
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
      store.notice = message(error);
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
      <h2 class="truncate">{title}</h2>
    </header>
    <div class="sheet-scroll">
      <div class="hero">
        <BookCover {title} src={coverUrl(coverId)} width={72} height={108} />
        <div class="post-column">
          <h3>{title}</h3>
          {#if info?.subtitle}<p class="meta">{info.subtitle}</p>{/if}
          <p class="meta">{author}{year != null ? ` · ${year}` : ''}</p>
          {#if badgeLabel}
            <p class="meta badge"><span class="dot {tone}"></span>{badgeLabel}</p>
          {/if}
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
            {#if detailRequest?.releaseTitle}
              <p class="meta truncate">{detailRequest.releaseTitle}</p>
            {/if}
          {:else if phase === 'retrying'}
            {#if liveRequest?.error}<p>{liveRequest.error}</p>{/if}
            <p class="meta">Trying another release…</p>
          {:else}
            <p>Checking and importing…</p>
          {/if}
          <button class="text-button danger" type="button" disabled={acting} onclick={() => { void cancel(); }}>
            Cancel request
          </button>
          {#if liveRequest}
            <p class="meta">Requested {relativeTime(liveRequest.createdAt)}</p>
          {/if}
        {:else if phase === 'failed' && liveRequest}
          <p>{liveRequest.error ?? 'The download failed.'}</p>
          {#if detailRequest}
            <p class="meta">Tried {detailRequest.attempts} of {detailRequest.maxAttempts} releases</p>
          {/if}
          <button class="pill" type="button" disabled={acting || atCap} onclick={() => { void retry(); }}>Retry</button>
          {#if atCap}<p class="meta">You have {detail?.status.cap} requests in progress</p>{/if}
        {:else if copy || phase === 'imported'}
          <p>{copy ? copyLabel(copy, 'sheet') : 'On your shelf'}</p>
          <button class="pill" type="button" disabled={!bookId} onclick={openBook}>Open book</button>
        {:else if phase === 'canceled'}
          <button class="pill" type="button" disabled={acting || atCap} onclick={() => { void retry(); }}>
            Request again
          </button>
          {#if atCap}<p class="meta">You have {detail?.status.cap} requests in progress</p>{/if}
        {:else}
          <button class="pill" type="button" disabled={acting || atCap} onclick={() => { void requestBook(); }}>
            Request
          </button>
          {#if atCap}<p class="meta">You have {detail?.status.cap} requests in progress</p>{/if}
          {#if info && !info.hasAcceptedEdition}
            <p class="meta">No English edition listed on Open Library</p>
          {/if}
        {/if}
        {#if detail && detail.status.otherRequests.length > 0}
          <h3 class="also">Also requested by</h3>
          <ul>
            {#each detail.status.otherRequests as row (row.id)}
              <li>@{row.requester} · {stateLabel(row)} · {relativeTime(row.updatedAt)}</li>
            {/each}
          </ul>
        {/if}
      </section>

      {#if loading && !info}
        <div class="about" aria-busy="true">
          <p class="sr-only" role="status">Loading details…</p>
          <span class="bone"></span>
          <span class="bone short"></span>
          <span class="bone"></span>
        </div>
      {:else if detailError}
        <p class="muted retry-line">
          {detailError}
          <button type="button" class="text-button" onclick={() => attempt += 1}>Try again</button>
        </p>
      {:else if detail && !info}
        <p class="muted retry-line">
          Couldn't load details from Open Library.
          <button type="button" class="text-button" onclick={() => attempt += 1}>Try again</button>
        </p>
      {:else if info}
        <div class="about">
          {#if info.description}
            <p class="description" class:clamped={!expanded} use:measure>{info.description}</p>
            {#if overflow || expanded}
              <button type="button" class="text-button" onclick={() => expanded = !expanded}>
                {expanded ? 'Less' : 'More'}
              </button>
            {/if}
          {/if}
          {#if info.series.length}
            <p>Series: {info.series.join(', ')}</p>
          {/if}
          {#if info.awards.length}
            <h3 class="also">Awards</h3>
            <ul class="awards">
              {#each info.awards as award (award)}<li>{award}</li>{/each}
            </ul>
          {/if}
          {#if info.subjects.length}
            <ul class="chips">
              {#each info.subjects as subject (subject)}<li>{subject}</li>{/each}
            </ul>
          {/if}
          {#if facts(info)}<p class="facts">{facts(info)}</p>{/if}
          {#if editionLine(info.languages)}<p class="meta">{editionLine(info.languages)}</p>{/if}
          <a
            href="https://openlibrary.org{info.canonicalKey}"
            target="_blank"
            rel="noopener"
          >View on Open Library</a>
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
    padding: 16px 16px 8px;
  }

  h3 {
    font-size: 1.25rem;
    line-height: 1.25;
    font-weight: 700;
  }

  .hero .meta {
    margin-top: 4px;
  }

  .badge {
    display: flex;
    align-items: center;
    gap: 6px;
  }

  .dot {
    width: 8px;
    height: 8px;
    flex: 0 0 8px;
    border-radius: 50%;
    background: var(--accent);
  }

  .dot.shelf { background: var(--green); }
  .dot.failed { background: var(--like); }
  .dot.server { background: var(--accent); }
  .dot.progress { background: var(--mute); }

  .status {
    margin: 8px 16px 16px;
    padding: 12px;
    border: .6px solid var(--line);
    border-radius: 12px;
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: 8px;
  }

  .status p {
    line-height: 1.4;
  }

  .status .pill, .status .text-button {
    margin-top: 4px;
  }

  progress {
    width: 100%;
    height: 4px;
    border: 0;
    background: var(--line);
    accent-color: var(--accent);
  }

  progress::-webkit-progress-bar { background: var(--line); }
  progress::-webkit-progress-value { background: var(--accent); }
  progress::-moz-progress-bar { background: var(--accent); }

  .also {
    font-size: .8125rem;
    line-height: 1.3;
    color: var(--mute);
    font-weight: 700;
    margin-top: 4px;
  }

  .status ul, .awards {
    margin: 0;
    padding: 0;
    list-style: none;
  }

  .status li, .awards li {
    font-size: .9375rem;
    line-height: 1.4;
  }

  .about {
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: 12px;
    padding: 0 16px 16px;
  }

  .description {
    line-height: 1.45;
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

  .facts {
    line-height: 1.4;
  }

  .chips {
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
    margin: 0;
    padding: 0;
    list-style: none;
  }

  .chips li {
    border: .6px solid var(--line);
    border-radius: 999px;
    padding: 4px 10px;
    font-size: .8125rem;
    line-height: 1.3;
  }

  .about a {
    color: var(--accent);
    font-weight: 700;
  }

  .retry-line {
    padding: 0 16px 16px;
  }

  .retry-line .text-button {
    display: inline;
    min-height: 0;
    padding: 0;
  }

  .bone {
    display: block;
    height: 14px;
    width: 100%;
    border-radius: 4px;
    background: color-mix(in srgb, var(--mute) 16%, var(--ground));
  }

  .bone.short {
    width: 64%;
  }
</style>
