/** Reset and verify links carry their token after `#token=`, so it never reaches a server log. */
export function linkToken(url: URL | string): string | null {
  let hash: string;
  try {
    hash = new URL(String(url)).hash;
  } catch {
    return null;
  }
  const token = new URLSearchParams(hash.replace(/^#/, '')).get('token')?.trim() ?? '';
  return token.length > 0 ? token : null;
}

/** The same URL without its hash, for `history.replaceState`. */
export function withoutHash(url: URL | string): string {
  const next = new URL(String(url));
  next.hash = '';
  return next.pathname + next.search;
}
