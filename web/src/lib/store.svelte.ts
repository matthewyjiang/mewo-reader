import { HostedClient } from './api';
import type {
  AuthResponse, BookRecord, CommentIndex, FeedPost, PostComment, PostHit, ProfileReply,
} from './models';
import { readStored, writeStored } from './storage';

export const message = (error: unknown): string => error instanceof Error
  ? error.message : 'Something went wrong. Try again.';

const savedSession = readStored<AuthResponse | null>('mewo.session', null);
const initialSession = savedSession
  && typeof savedSession.token === 'string' && typeof savedSession.username === 'string'
  ? savedSession : null;

/** One hosted shelf. Logout wipes private caches and invalidates pending responses. */
class HostedStore {
  session = $state<AuthResponse | null>(initialSession);
  books = $state<BookRecord[]>([]);
  // Parsed text is immutable. Avoid deep proxies for thousands of paragraphs.
  feeds = $state.raw<Record<string, FeedPost[]>>({});
  likes = $state<Record<string, string[]>>({});
  indexes = $state<Record<string, CommentIndex>>({});
  covers = $state<Record<string, string | null>>({});
  notice = $state('');
  api = new HostedClient(
    () => this.session?.token,
    () => this.clear('Your session expired. Sign in again.'),
  );

  private epoch = 0;
  private pendingFeeds = new Map<string, Promise<FeedPost[]>>();
  private pendingCovers = new Map<string, Promise<void>>();
  private progressTimers = new Map<string, ReturnType<typeof setTimeout>>();
  private pendingProgress = new Map<string, number>();
  private progressQueues = new Map<string, Promise<void>>();

  clear(notice = ''): void {
    this.epoch++;
    this.session = null;
    writeStored('mewo.session', null);
    this.books = [];
    this.feeds = {};
    this.likes = {};
    this.indexes = {};
    Object.values(this.covers).forEach((url) => {
      if (url) URL.revokeObjectURL(url);
    });
    this.covers = {};
    this.pendingFeeds.clear();
    this.pendingCovers.clear();
    this.progressTimers.forEach(clearTimeout);
    this.progressTimers.clear();
    this.pendingProgress.clear();
    this.progressQueues.clear();
    this.notice = notice;
  }

  async signIn(mode: 'login' | 'register', username: string, password: string): Promise<void> {
    const session = await this.api.auth(mode, username.trim(), password);
    this.clear();
    this.session = session;
    writeStored('mewo.session', session);
  }

  async signOut(): Promise<void> {
    this.flushProgress();
    await Promise.all(this.progressQueues.values());
    try { await this.api.logout(); }
    finally { this.clear(); }
  }

  async load(): Promise<void> {
    const epoch = this.epoch;
    const books = await this.api.library();
    if (epoch !== this.epoch) return;
    this.books = books.sort((a, b) => b.importedAt - a.importedAt);
    // Covers are independent of shelf loading and may be absent.
    for (const book of books) void this.loadCover(book.id).catch(() => {});
  }

  replaceBook(book: BookRecord): void {
    this.books = [...this.books.filter((row) => row.id !== book.id), book]
      .sort((a, b) => b.importedAt - a.importedAt);
  }

  async book(id: string): Promise<BookRecord> {
    const existing = this.books.find((book) => book.id === id);
    if (existing) return existing;
    const epoch = this.epoch;
    const book = await this.api.book(id);
    if (epoch === this.epoch) this.replaceBook(book);
    return book;
  }

  async feed(id: string): Promise<FeedPost[]> {
    if (this.feeds[id]) return this.feeds[id];
    const pending = this.pendingFeeds.get(id);
    if (pending) return pending;
    const epoch = this.epoch;
    const promise = this.api.posts(id).then((posts) => {
      if (epoch === this.epoch) this.feeds = { ...this.feeds, [id]: posts };
      return posts;
    }).finally(() => {
      if (epoch === this.epoch) this.pendingFeeds.delete(id);
    });
    this.pendingFeeds.set(id, promise);
    return promise;
  }

  async loadCover(id: string): Promise<void> {
    if (id in this.covers) return;
    const pending = this.pendingCovers.get(id);
    if (pending) return pending;
    const epoch = this.epoch;
    const promise = this.api.cover(id).then((blob) => {
      if (epoch === this.epoch) this.covers[id] = blob ? URL.createObjectURL(blob) : null;
    }).finally(() => {
      if (epoch === this.epoch) this.pendingCovers.delete(id);
    });
    this.pendingCovers.set(id, promise);
    return promise;
  }

  async social(id: string): Promise<void> {
    const epoch = this.epoch;
    const [likes, index] = await Promise.all([
      this.api.likes(id), this.api.commentIndex(id),
    ]);
    if (epoch === this.epoch) {
      this.likes[id] = likes;
      this.indexes[id] = index;
    }
  }

  async toggleLike(id: string, postId: string): Promise<void> {
    const epoch = this.epoch;
    const likes = await this.api.toggleLike(id, postId);
    if (epoch === this.epoch) this.likes[id] = likes;
  }

  afterThread(id: string, postId: string, comments: PostComment[]): void {
    const current = this.indexes[id] ?? { counts: {}, mine: [] };
    this.indexes[id] = {
      counts: { ...current.counts, [postId]: comments.length },
      mine: [
        ...current.mine.filter((value) => value !== postId),
        ...(comments.some((comment) => comment.mine) ? [postId] : []),
      ],
    };
  }

  search(query: string): { books: BookRecord[]; posts: PostHit[] } {
    const q = query.trim().toLocaleLowerCase();
    if (!q) return { books: [], posts: [] };
    return {
      books: this.books.filter((book) => [book.title, book.author, book.handle]
        .some((value) => value.toLocaleLowerCase().includes(q))),
      posts: this.books.flatMap((book) => (this.feeds[book.id] ?? [])
        .filter((post) => post.text.toLocaleLowerCase().includes(q)
          || post.chapter.toLocaleLowerCase().includes(q))
        .map((post) => ({ book, post }))),
    };
  }

  async loadLikes(): Promise<void> {
    const epoch = this.epoch;
    // Only fetch text for books with likes, including likes made on a phone.
    for (const book of this.books) {
      const ids = await this.api.likes(book.id);
      if (epoch !== this.epoch) return;
      this.likes[book.id] = ids;
      if (ids.length) await Promise.all([this.feed(book.id), this.social(book.id)]);
      if (epoch !== this.epoch) return;
    }
  }

  likedPosts(): PostHit[] {
    return this.books.flatMap((book) => {
      const ids = new Set(this.likes[book.id] ?? []);
      return (this.feeds[book.id] ?? [])
        .filter((post) => ids.has(post.id))
        .map((post) => ({ book, post }));
    });
  }

  async profile(username: string): Promise<ProfileReply[]> {
    const epoch = this.epoch;
    const remote = await this.api.profile(username);
    const replies: ProfileReply[] = [];
    for (const row of remote.replies) {
      if (epoch !== this.epoch) return [];
      const book = this.books.find((book) => book.id === row.bookId);
      // Deleted books have no original line to show.
      if (!book) continue;
      const posts = await this.feed(book.id);
      const post = posts.find((post) => post.id === row.postId);
      if (post) replies.push({ book, post, comment: row.comment });
    }
    for (const id of new Set(replies.map((reply) => reply.book.id))) {
      if (epoch !== this.epoch) return [];
      await this.social(id);
    }
    return replies.sort((a, b) => b.comment.createdAt - a.comment.createdAt);
  }

  async upload(
    file: File,
    sample: boolean,
    progress: (percent: number) => void,
  ): Promise<BookRecord> {
    const epoch = this.epoch;
    const book = await this.api.upload(file, sample, progress);
    if (epoch === this.epoch) {
      this.replaceBook(book);
      void this.loadCover(book.id).catch(() => {});
    }
    return book;
  }

  async remove(book: BookRecord): Promise<void> {
    const epoch = this.epoch;
    await this.api.remove(book.id);
    if (epoch !== this.epoch) return;
    clearTimeout(this.progressTimers.get(book.id));
    this.progressTimers.delete(book.id);
    this.pendingProgress.delete(book.id);
    this.books = this.books.filter((row) => row.id !== book.id);
    const feeds = { ...this.feeds };
    delete feeds[book.id];
    this.feeds = feeds;
    delete this.likes[book.id];
    delete this.indexes[book.id];
    const cover = this.covers[book.id];
    if (cover) URL.revokeObjectURL(cover);
    delete this.covers[book.id];
  }

  /** Save top-visible posts after scrolling settles; flush on navigation/backgrounding.
   * Unlike the phones' per-index writes, web coalesces events over their existing
   * 200ms chrome transition. Writes are serialized per book to avoid stale races. */
  saveProgress(id: string, index: number): void {
    const book = this.books.find((book) => book.id === id);
    if (!book || book.progressIndex === index) return;
    this.replaceBook({ ...book, progressIndex: index });
    this.pendingProgress.set(id, index);
    clearTimeout(this.progressTimers.get(id));
    this.progressTimers.set(id, setTimeout(() => this.flushProgress(id), 200));
  }

  flushProgress(onlyId?: string): void {
    for (const [id, index] of this.pendingProgress) {
      if (onlyId && id !== onlyId) continue;
      clearTimeout(this.progressTimers.get(id));
      this.progressTimers.delete(id);
      this.pendingProgress.delete(id);
      const epoch = this.epoch;
      const previous = this.progressQueues.get(id) ?? Promise.resolve();
      const next = previous.then(async () => {
        if (epoch !== this.epoch) return;
        try {
          await this.api.progress(id, index, true);
        } catch (error) {
          if (epoch === this.epoch) {
            this.notice = `Reading place was not saved: ${message(error)}`;
          }
        }
      });
      this.progressQueues.set(id, next);
    }
  }
}

export const store = new HostedStore();
