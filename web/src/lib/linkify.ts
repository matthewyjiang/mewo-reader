/**
 * Link spans for post text. The iOS port should mirror this rule.
 *
 * Find the earliest `http://` or `https://` from the cursor, including one
 * sitting against the previous word (`supervision:https://…`). End at the
 * first Unicode whitespace character. Then strip trailing `.,;:!?)]}'"`, but
 * keep a `)` when the URL before that parenthesis contains more `(` than `)`.
 * Keep a trailing `/`. A match that does not extend past the scheme is not a
 * link. Matching is case-sensitive and does not require a word boundary.
 * Indexes are UTF-16 code units, the same as a JavaScript string and a Kotlin
 * `Char` index.
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
      if (text[end - 1] === ')' && unmatchedOpen(text, start, end - 1)) break;
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

function unmatchedOpen(text: string, start: number, end: number): boolean {
  let open = 0;
  let close = 0;
  for (let index = start; index < end; index += 1) {
    if (text[index] === '(') open += 1;
    else if (text[index] === ')') close += 1;
  }
  return open > close;
}
