import type { BookRequest } from './models';

/** Searching, waiting, downloading, on the shelf, failed, or canceled. */
export function stateLabel(request: Pick<BookRequest, 'state' | 'progress' | 'error'>): string {
  if (request.state === 'requested') {
    return request.error === 'No release yet.' ? 'Waiting for a release' : 'Searching';
  }
  if (request.state === 'downloading') return `Downloading ${request.progress}%`;
  if (request.state === 'imported') return 'On your shelf';
  if (request.state === 'failed') return `Failed: ${request.error ?? ''}`;
  return 'Canceled';
}

/** The catalog row's trailing action. */
export function rowAction(request: BookRequest | null): 'request' | 'pending' | 'open' | 'retry' {
  if (!request) return 'request';
  if (request.state === 'requested' || request.state === 'downloading') return 'pending';
  if (request.state === 'imported' && request.bookId) return 'open';
  return 'retry';
}

/** Open Library cover CDN, medium JPEG. `default=false` makes a missing cover
 * 404 instead of a 1×1 GIF. Not proxied. */
export function coverUrl(coverId: number | null): string | null {
  if (coverId == null || coverId < 0) return null;
  return `https://covers.openlibrary.org/b/id/${coverId}-M.jpg?default=false`;
}
