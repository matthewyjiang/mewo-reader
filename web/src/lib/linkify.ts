/**
 * Link spans for post text. The iOS port should mirror this rule.
 *
 * Find the earliest `http://` or `https://` from the cursor, including one
 * sitting against the previous word (`supervision:https://…`). End at the
 * first Unicode whitespace character. Then strip trailing `.,;:!?)]}'"`.
 * Keep a `)` or `]` when the URL before that closer has more of its opener
 * than its closer, so `http://[::1]` keeps the bracket and an inner `)` in
 * `Hopfield_network_(neural)` stays. Keep a trailing `/`. A match that does
 * not extend past the scheme is not a link. Matching is case-sensitive and
 * does not require a word boundary. Indexes are UTF-16 code units, the same
 * as a JavaScript string and a Kotlin `Char` index.
 */
export interface LinkSpan {
  start: number;
  end: number;
  href: string;
}

const TRAILING = ".,;:!?)]}'\"";

export function linkSpans(text: string): LinkSpan[] {
  const spans: LinkSpan[] = [];
  let cursor = 0;
  while (cursor < text.length) {
    const https = text.indexOf('https://', cursor);
    const http = text.indexOf('http://', cursor);
    const start = https < 0 ? http : http < 0 ? https : Math.min(https, http);
    if (start < 0) break;
    const scheme = text.startsWith('https://', start) ? 'https://' : 'http://';
    let end = start;
    while (end < text.length && !isWhitespace(text[end]!)) end += 1;
    while (end > start + scheme.length && TRAILING.includes(text[end - 1]!)) {
      if (keepCloser(text, start, end)) break;
      end -= 1;
    }
    if (end > start + scheme.length) {
      spans.push({ start, end, href: text.slice(start, end) });
      cursor = end;
    } else {
      cursor = start + scheme.length;
    }
  }
  return spans;
}

function isWhitespace(char: string): boolean {
  return /\s/u.test(char);
}

function keepCloser(text: string, start: number, end: number): boolean {
  const closer = text[end - 1];
  if (closer === ')') return unmatched(text, start, end - 1, '(', ')');
  if (closer === ']') return unmatched(text, start, end - 1, '[', ']');
  return false;
}

function unmatched(
  text: string,
  start: number,
  end: number,
  openChar: string,
  closeChar: string,
): boolean {
  let open = 0;
  let close = 0;
  for (let index = start; index < end; index += 1) {
    if (text[index] === openChar) open += 1;
    else if (text[index] === closeChar) close += 1;
  }
  return open > close;
}
