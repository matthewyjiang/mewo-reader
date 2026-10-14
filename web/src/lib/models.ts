/** Hosted wire types match the phone models; the server owns EPUB parsing. */
export interface BookRecord {
  id: string;
  title: string;
  author: string;
  handle: string;
  importedAt: number;
  postCount: number;
  progressIndex: number;
  isSample: boolean;
  mine: boolean;
  /** Owner chose to show this book on other accounts' shelves. */
  shared: boolean;
  /** Username of the account that added the book. */
  ownerUsername: string;
}

export interface FeedPost {
  id: string;
  kind: 'Heading' | 'Body';
  text: string;
  chapter: string;
}

export interface PostComment {
  id: string;
  username: string;
  text: string;
  createdAt: number;
  mine: boolean;
}

export interface CommentIndex {
  counts: Record<string, number>;
  mine: string[];
}

export interface AuthResponse {
  token: string;
  username: string;
}

export interface PostHit {
  book: BookRecord;
  post: FeedPost;
}

export interface ProfileReply extends PostHit {
  comment: PostComment;
}

export interface ProfileReplies {
  username: string;
  replies: { bookId: string; postId: string; comment: PostComment }[];
}

// A one-post book finishes only on the end card, not when first opened.
export const isFinished = (book: BookRecord): boolean =>
  book.postCount > 0 && book.progressIndex >= Math.max(1, book.postCount - 1);

/** The sample stays readable by every account. Other books need the owner to share them. */
export const visibleToOthers = (book: BookRecord): boolean => book.shared || book.isSample;

export function shelfGroups(books: BookRecord[]): {
  mine: BookRecord[];
  sharedWithMe: BookRecord[];
  finished: BookRecord[];
} {
  const finished = books.filter(isFinished);
  const active = books.filter((book) => !isFinished(book));
  return {
    mine: active.filter((book) => book.mine),
    sharedWithMe: active.filter((book) => !book.mine),
    finished,
  };
}

export function statusLine(book: BookRecord): string {
  const count = book.postCount ? `${book.postCount.toLocaleString()} posts` : 'Ready to read';
  if (isFinished(book)) return `${count} · Finished`;
  if (book.postCount > 0 && book.progressIndex > 0) {
    const percent = Math.min(100, Math.max(1,
      Math.floor((book.progressIndex + 1) * 100 / book.postCount)));
    return `${count} · ${percent}% read`;
  }
  return count;
}

export function currentHeadingIndex(
  headings: { index: number }[],
  visibleIndex: number,
): number {
  let current = 0;
  for (let i = 0; i < headings.length && headings[i].index <= visibleIndex; i++) current = i;
  return current;
}

export function relativeTime(createdAt: number): string {
  const minutes = Math.max(0, Math.floor((Date.now() - createdAt) / 60000));
  if (minutes < 1) return 'now';
  if (minutes < 60) return `${minutes}m`;
  const hours = Math.floor(minutes / 60);
  return hours < 24 ? `${hours}h` : `${Math.floor(hours / 24)}d`;
}
