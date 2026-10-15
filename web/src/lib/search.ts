import type { BookRecord, FeedPost, PostHit } from './models';

export interface SearchResults {
  books: BookRecord[];
  posts: PostHit[];
}

// Leave half a 60 Hz frame for input, layout, and paint between search slices.
const sliceMs = 1000 / 60 / 2;
const yieldToInput = (): Promise<void> => new Promise((resolve) => setTimeout(resolve, 0));

/** Search cached text without holding the UI thread. Aborted searches publish nothing. */
export async function searchLibrary(
  books: BookRecord[],
  feeds: Record<string, FeedPost[]>,
  query: string,
  signal: AbortSignal,
): Promise<SearchResults | null> {
  const q = query.trim().toLocaleLowerCase();
  const results: SearchResults = { books: [], posts: [] };
  if (!q) return signal.aborted ? null : results;

  // Let the field update before scanning, including the first keystroke.
  await yieldToInput();
  let deadline = performance.now() + sliceMs;
  for (const book of books) {
    if (performance.now() >= deadline) {
      await yieldToInput();
      deadline = performance.now() + sliceMs;
    }
    if (signal.aborted) return null;
    if ([book.title, book.author, book.handle]
      .some((value) => value.toLocaleLowerCase().includes(q))) results.books.push(book);
    for (const post of feeds[book.id] ?? []) {
      if (performance.now() >= deadline) {
        await yieldToInput();
        if (signal.aborted) return null;
        deadline = performance.now() + sliceMs;
      }
      if (post.text.toLocaleLowerCase().includes(q)
        || post.chapter.toLocaleLowerCase().includes(q)) results.posts.push({ book, post });
    }
  }
  return signal.aborted ? null : results;
}
