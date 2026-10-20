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
  /** Posts with at least one repost. Zero counts are omitted, same as `counts`. */
  reposts?: Record<string, number>;
  /** Post ids this viewer has reposted. Same shape as `mine`. */
  reposted?: string[];
}

export interface RepostState {
  reposts: number;
  reposted: boolean;
}

export interface AccountHit {
  username: string;
  mine: boolean;
}

export type SearchKind = 'accounts' | 'books' | 'posts' | 'catalog';

export type RequestStateName = 'requested' | 'downloading' | 'imported' | 'failed' | 'canceled';

export interface BookRequest {
  id: string;
  workKey: string;
  title: string;
  author: string;
  year: number | null;
  coverId: number | null;
  state: RequestStateName;
  progress: number;
  error: string | null;
  bookId: string | null;
  requester: string;
  createdAt: number;
  updatedAt: number;
}

export interface CatalogWork {
  workKey: string;
  title: string;
  author: string;
  year: number | null;
  coverId: number | null;
  request: BookRequest | null;
}

export type SearchPage =
  | { type: 'accounts'; items: AccountHit[]; nextCursor: string | null }
  | { type: 'books'; items: BookRecord[]; nextCursor: string | null }
  | { type: 'posts'; items: PostHit[]; nextCursor: string | null }
  | { type: 'catalog'; items: CatalogWork[]; nextCursor: string | null };

export interface ProfileRecord {
  username: string;
  joinedAt: number;
  postCount: number;
  repostCount: number;
  mine: boolean;
}

export interface ProfilePost extends PostHit {
  comment: PostComment;
}

export interface ProfilePostsPage {
  items: ProfilePost[];
  nextCursor: string | null;
}

export interface ProfileRepost extends PostHit {
  repostedAt: number;
}

export interface ProfileRepostsPage {
  items: ProfileRepost[];
  nextCursor: string | null;
}

export interface ProfileBooks {
  items: BookRecord[];
}

export type HomeFeedItem =
  | {
    type: 'comment';
    actor: { username: string };
    book: BookRecord;
    post: FeedPost;
    comment: PostComment;
    at: number;
  }
  | {
    type: 'repost';
    actor: { username: string };
    book: BookRecord;
    post: FeedPost;
    at: number;
  };

export interface HomeFeedPage {
  items: HomeFeedItem[];
  nextCursor: string | null;
}

export interface AuthResponse {
  token: string;
  username: string;
}

export interface Me {
  username: string;
  isAdmin: boolean;
  canRequest: boolean;
}

export interface ServerSettings {
  allowSignup: boolean;
  maxEpubBytes: number;
  requestsEnabled: boolean;
  prowlarrUrl: string;
  sabnzbdUrl: string;
  sabnzbdCategory: string;
  /** From `MEWO_DOWNLOADS_DIR`. Not a setting; PATCH rejects it. */
  downloadsDir: string | null;
  secrets: { prowlarrApiKey: boolean; sabnzbdApiKey: boolean };
}

export type SettingsPatch = Partial<Omit<ServerSettings, 'secrets' | 'downloadsDir'>> & {
  prowlarrApiKey?: string | null;
  sabnzbdApiKey?: string | null;
};

export interface AdminUser {
  username: string;
  joinedAt: number;
  isAdmin: boolean;
  books: number;
  mine: boolean;
}

export interface ServerInfo {
  users: number;
  books: number;
  comments: number;
  reposts: number;
  storageBytes: number;
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

const months = ['Jan', 'Feb', 'Mar', 'Apr', 'May', 'Jun', 'Jul', 'Aug', 'Sep', 'Oct', 'Nov', 'Dec'];

/** `Joined Mon YYYY` from `users.created_at` milliseconds, in UTC. */
export function joinedLabel(joinedAt: number): string {
  const date = new Date(joinedAt);
  return `Joined ${months[date.getUTCMonth()]} ${date.getUTCFullYear()}`;
}

/** `now`, `5m`, `3h`, then a UTC date: `Oct 3` this year, `Oct 3, 2024` otherwise. */
export function relativeTime(createdAt: number, now = Date.now()): string {
  const minutes = Math.max(0, Math.floor((now - createdAt) / 60000));
  if (minutes < 1) return 'now';
  if (minutes < 60) return `${minutes}m`;
  const hours = Math.floor(minutes / 60);
  if (hours < 24) return `${hours}h`;
  const date = new Date(createdAt);
  const label = `${months[date.getUTCMonth()]} ${date.getUTCDate()}`;
  const year = date.getUTCFullYear();
  return year === new Date(now).getUTCFullYear() ? label : `${label}, ${year}`;
}
