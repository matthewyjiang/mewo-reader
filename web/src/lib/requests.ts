import type { BookRequest, CatalogWork, RequestPhase, ServerCopy } from './models';

const PHASES = new Set<RequestPhase>([
  'searching', 'waiting', 'no_match', 'held', 'retrying', 'downloading', 'importing',
  'failed', 'canceled', 'imported',
]);

const HOLD_SERVICES = ['SABnzbd', 'Prowlarr', 'Open Library'] as const;

type LabeledRequest = Pick<BookRequest, 'state' | 'progress' | 'error'> & { phase?: RequestPhase };

function isPhase(value: string | undefined): value is RequestPhase {
  return value != null && PHASES.has(value as RequestPhase);
}

/** Service name from a held-upstream error, such as `SABnzbd: down`. */
export function holdService(error: string | null | undefined): string | null {
  if (!error) return null;
  for (const service of HOLD_SERVICES) {
    if (error.startsWith(`${service}:`)) return service;
  }
  return null;
}

/** Title inside `No matching release (closest: …, a different book)`. */
export function closestTitle(error: string | null | undefined): string | null {
  if (!error) return null;
  const match = error.match(/\(closest: (.*?), a different book\)/);
  return match ? match[1] : null;
}

function labelForPhase(phase: RequestPhase, request: Pick<BookRequest, 'progress' | 'error'>): string {
  switch (phase) {
    case 'searching': return 'Searching';
    case 'waiting': return 'Waiting for a release';
    case 'no_match': return 'No matching release';
    case 'held': {
      const service = holdService(request.error);
      return service ? `Waiting on ${service}` : 'Waiting';
    }
    case 'retrying': return 'Retrying';
    case 'downloading': return `Downloading ${request.progress}%`;
    case 'importing': return 'Importing';
    case 'imported': return 'On your shelf';
    case 'failed': return `Failed: ${request.error ?? ''}`;
    case 'canceled': return 'Canceled';
  }
}

/** Searching, waiting, downloading, on the shelf, failed, or canceled.
 *  A server `phase` wins. Without one, the list wire keeps the older labels. */
export function stateLabel(request: LabeledRequest): string {
  if (isPhase(request.phase)) return labelForPhase(request.phase, request);
  if (request.state === 'requested') {
    return request.error === 'No release yet.' ? 'Waiting for a release' : 'Searching';
  }
  if (request.state === 'downloading') return `Downloading ${request.progress}%`;
  if (request.state === 'imported') return 'On your shelf';
  if (request.state === 'failed') return `Failed: ${request.error ?? ''}`;
  return 'Canceled';
}

/** Same rule as the server's `phase`, for a list row that has no `phase` yet. */
export function requestPhase(request: LabeledRequest): RequestPhase {
  if (isPhase(request.phase)) return request.phase;
  const error = request.error ?? '';
  if (request.state === 'imported') return 'imported';
  if (request.state === 'failed') return 'failed';
  if (request.state === 'canceled') return 'canceled';
  if (request.state === 'downloading') {
    if (holdService(error)) return 'held';
    if (request.progress >= 100) return 'importing';
    return 'downloading';
  }
  if (request.state === 'requested') {
    if (error === 'No release yet.') return 'waiting';
    if (error.startsWith('No matching release')) return 'no_match';
    if (holdService(error)) return 'held';
    if (error !== '') return 'retrying';
  }
  return 'searching';
}

export function isLivePhase(phase: RequestPhase | undefined): boolean {
  return phase === 'searching' || phase === 'waiting' || phase === 'no_match'
    || phase === 'held' || phase === 'retrying' || phase === 'downloading' || phase === 'importing';
}

/** The catalog row's trailing action. A copy the viewer can open wins over Request,
 *  except while their own request is still running or failed.
 *  `retry` is failed only. `again` is a new ask: canceled still posts /retry,
 *  and an imported row with no book posts /v1/requests. */
export function rowAction(
  request: BookRequest | null,
  onServer?: ServerCopy | null,
): 'request' | 'pending' | 'open' | 'retry' | 'again' {
  if (request && (request.state === 'requested' || request.state === 'downloading')) return 'pending';
  if (request?.state === 'failed') return 'retry';
  if (request?.state === 'imported' && request.bookId) return 'open';
  if (onServer?.bookId) return 'open';
  if (request?.state === 'canceled' || request?.state === 'imported') return 'again';
  return 'request';
}

const RAW_STATUS = /^(?:server returned \d+|http \d+)\b/i;

/** Failure copy for the catalog list and the sheet. The Try again control sits
 *  beside it, so the sentence itself never ends that way. */
export function catalogErrorText(status: number, message: string): string {
  if (status === 0) return "Couldn't reach the server. Check your connection.";
  if (status === 401) return 'Sign in again.';
  if (status === 403) return 'Requests are off.';
  if (status === 404) return 'That book is no longer in the catalog.';
  const trimmed = message.trim();
  if (!trimmed || RAW_STATUS.test(trimmed)) return "Couldn't load the catalog.";
  const sentence = trimmed.replace(/(?:^|\s)Try again\.$/, '').trim();
  return sentence || "Couldn't load the catalog.";
}

/** Held and owned copies say "On your shelf". Someone else's shared copy says "On server". */
export function openLabel(
  work: { onServer?: ServerCopy | null },
  request: BookRequest | null,
): string {
  if (request?.state === 'imported' || request?.bookId) return 'On your shelf';
  const copy = work.onServer;
  if (!copy || copy.mine || copy.held) return 'On your shelf';
  return 'On server';
}

/** A held copy never names its owner, even when `ownerUsername` is filled in. */
export function copyLabel(copy: ServerCopy, style: 'badge' | 'sheet'): string {
  if (copy.mine || copy.held) return 'On your shelf';
  if (copy.shared && copy.ownerUsername) {
    return style === 'badge'
      ? `On server · @${copy.ownerUsername}`
      : `On server, shared by @${copy.ownerUsername}`;
  }
  return 'On server';
}

/** Own active or failed request, then a copy on the server, then nothing. */
export function badge(
  work: Pick<CatalogWork, 'onServer'>,
  request: (LabeledRequest & Pick<BookRequest, 'bookId'>) | null,
): string | null {
  if (request && (request.state === 'requested' || request.state === 'downloading' || request.state === 'failed')) {
    return stateLabel(request);
  }
  if (request?.state === 'imported' && request.bookId) return 'On your shelf';
  if (work.onServer) return copyLabel(work.onServer, 'badge');
  return null;
}

export function badgeTone(label: string): 'shelf' | 'failed' | 'server' | 'progress' {
  if (label.startsWith('Failed')) return 'failed';
  if (label === 'On your shelf') return 'shelf';
  if (label.startsWith('On server')) return 'server';
  return 'progress';
}

export function catalogRowLabel(work: CatalogWork, request: BookRequest | null): string {
  const parts = [work.title];
  if (work.author) parts.push(work.author);
  if (work.year != null) parts.push(String(work.year));
  const status = badge(work, request);
  if (status) parts.push(status);
  return parts.join(', ');
}

export function catalogOlid(workKey: string): string {
  const slash = workKey.lastIndexOf('/');
  return slash >= 0 ? workKey.slice(slash + 1) : workKey;
}

/** Viewer's locale and time zone. The sheet says when the next search runs. */
export function formatCheck(at: number): string {
  return new Date(at).toLocaleString(undefined, {
    month: 'short',
    day: 'numeric',
    hour: 'numeric',
    minute: '2-digit',
  });
}

export function waitingCopy(nextCheckAt: number | null | undefined): string {
  if (nextCheckAt == null) return 'No release yet.';
  return `No release yet. Next search ${formatCheck(nextCheckAt)}`;
}

export function noMatchCopy(error: string | null | undefined, nextCheckAt: number | null | undefined): string {
  const closest = closestTitle(error);
  const found = closest ? `Only found a different book (${closest}).` : 'Only found a different book.';
  if (nextCheckAt == null) return found;
  return `${found} Searching again ${formatCheck(nextCheckAt)}.`;
}

export function heldCopy(error: string | null | undefined): string {
  const service = holdService(error);
  if (!service) return error ? `Waiting: ${error}` : 'Waiting on the download service.';
  const rest = error?.slice(service.length + 1).trim() ?? '';
  return rest ? `Waiting on ${service}: ${rest}` : `Waiting on ${service}`;
}

/** One muted line. The first three names, then +N for the rest. */
export function alsoRequested(names: string[]): string {
  const shown = names.slice(0, 3).map((name) => `@${name}`);
  const extra = names.length - shown.length;
  const list = shown.join(', ');
  return extra > 0 ? `Also requested by ${list} +${extra}` : `Also requested by ${list}`;
}

/** Year, length, and average. Edition count and rating count stay off this line. */
export function workStats(info: {
  year: number | null;
  pagesMedian: number | null;
  rating: { average: number } | null;
}): string {
  const parts: string[] = [];
  if (info.year != null) parts.push(`First published ${info.year}`);
  if (info.pagesMedian != null) parts.push(`~${info.pagesMedian.toLocaleString()} pages`);
  if (info.rating) parts.push(`★ ${info.rating.average.toFixed(1)}`);
  return parts.join(' · ');
}

/** Plain text. Subjects are not links, so they are not chips. */
export function subjectLine(subjects: string[]): string {
  return subjects.slice(0, 5).join(' · ');
}

/** Open Library cover CDN, medium JPEG. `default=false` makes a missing cover
 * 404 instead of a 1×1 GIF. Not proxied. */
export function coverUrl(coverId: number | null): string | null {
  if (coverId == null || coverId < 0) return null;
  return `https://covers.openlibrary.org/b/id/${coverId}-M.jpg?default=false`;
}
