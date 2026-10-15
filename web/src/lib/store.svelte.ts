import { HostedClient } from './api';
import type {
  AuthResponse, BookRecord, CommentIndex, FeedPost, PostComment, PostHit, ProfileReply,
} from './models';
import { PendingProgress } from './progress';
import { readStored, writeStored } from './storage';

export const message = (error: unknown): string => error instanceof Error
  ? error.message : 'Something went wrong. Try again.';

const savedSession = readStored<AuthResponse | null>('mewo.session', null);
const initialSession = savedSession
  && typeof savedSession.token === 'string' && typeof savedSession.username === 'string'
  ? savedSession : null;

/** Hosted library. Logout and book removal invalidate pending responses. */
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
  private removed = new Set<string>();
  private pendingFeeds = new Map<string, Promise<FeedPost[]>>();
  private pendingCovers = new Map<string, Promise<void>>();
  private postMaps = new Map<string, Map<string, FeedPost>>();
  private socialGeneration = new Map<string, number>();
  private socialRequests = new Map<string, number>();
  private mutationQueues = new Map<string, Promise<void>>();
  // Optimistic share writes are serialized per book. Rollback uses the last
  // value the server confirmed, and a newer toggle keeps the checkbox.
  private shareTails = new Map<string, Promise<void>>();
  private confirmedShare = new Map<string, boolean>();
  private progress = new PendingProgress(async (id, index, keepalive) => {
    const epoch = this.epoch;
    const response = await this.api.progress(id, index, keepalive);
    const next = await response.json() as BookRecord;
    const book = this.books.find((row) => row.id === id);
    if (this.valid(id, epoch) && book && Number.isInteger(next.postCount)) {
      this.replaceBook({ ...book, postCount: next.postCount });
    }
  }, (error) => { this.notice = `Reading place was not saved: ${message(error)}`; });

  private valid(id: string, epoch: number): boolean {
    return epoch === this.epoch && !this.removed.has(id);
  }

  clear(notice = ''): void {
    this.epoch++;
    this.session = null;
    writeStored('mewo.session', null);
    this.books = [];
    this.feeds = {};
    this.likes = {};
    this.indexes = {};
    Object.values(this.covers).forEach((url) => { if (url) URL.revokeObjectURL(url); });
    this.covers = {};
    this.pendingFeeds.clear();
    this.pendingCovers.clear();
    this.postMaps.clear();
    this.removed.clear();
    this.socialGeneration.clear();
    this.socialRequests.clear();
    this.mutationQueues.clear();
    this.shareTails.clear();
    this.confirmedShare.clear();
    this.progress.clear();
    this.notice = notice;
  }

  async signIn(mode: 'login' | 'register', username: string, password: string): Promise<void> {
    const session = await this.api.auth(mode, username.trim(), password);
    this.clear();
    this.session = session;
    writeStored('mewo.session', session);
  }

  async signOut(): Promise<void> {
    await this.progress.settle();
    try { await this.api.logout(); }
    finally { this.clear(); }
  }

  async load(): Promise<void> {
    const epoch = this.epoch;
    const books = await this.api.library();
    if (epoch !== this.epoch) return;
    this.books = books.filter((book) => !this.removed.has(book.id))
      .sort((a, b) => b.importedAt - a.importedAt);
    if (this.session) this.progress.start(this.session.username);
    for (const [id, index] of this.progress.entries()) {
      const book = this.books.find((row) => row.id === id);
      if (book) this.replaceBook({ ...book, progressIndex: index });
      else this.progress.remove(id);
    }
    this.flushProgress();
    for (const book of this.books) void this.loadCover(book.id).catch(() => {});
  }

  replaceBook(book: BookRecord): void {
    if (this.removed.has(book.id)) return;
    this.books = [...this.books.filter((row) => row.id !== book.id), book]
      .sort((a, b) => b.importedAt - a.importedAt);
  }

  async book(id: string): Promise<BookRecord> {
    const existing = this.books.find((book) => book.id === id);
    if (existing) return existing;
    const epoch = this.epoch;
    const book = await this.api.book(id);
    if (this.valid(id, epoch)) this.replaceBook(book);
    return book;
  }

  async feed(id: string): Promise<FeedPost[]> {
    if (this.feeds[id]) return this.feeds[id];
    const pending = this.pendingFeeds.get(id);
    if (pending) return pending;
    const epoch = this.epoch;
    const promise = this.api.posts(id).then((posts) => {
      if (this.valid(id, epoch)) {
        this.feeds = { ...this.feeds, [id]: posts };
        this.postMaps.set(id, new Map(posts.map((post) => [post.id, post])));
        const book = this.books.find((row) => row.id === id);
        if (book) this.replaceBook({ ...book, postCount: posts.length });
      }
      return posts;
    }).finally(() => {
      if (epoch === this.epoch) this.pendingFeeds.delete(id);
    });
    this.pendingFeeds.set(id, promise);
    return promise;
  }

  async loadCover(id: string): Promise<void> {
    if (id in this.covers || this.removed.has(id)) return;
    const pending = this.pendingCovers.get(id);
    if (pending) return pending;
    const epoch = this.epoch;
    const promise = this.api.cover(id).then((blob) => {
      // Check before allocating a URL, not just before publishing it.
      if (this.valid(id, epoch)) this.covers[id] = blob ? URL.createObjectURL(blob) : null;
    }).finally(() => {
      if (epoch === this.epoch) this.pendingCovers.delete(id);
    });
    this.pendingCovers.set(id, promise);
    return promise;
  }

  private bumpSocial(id: string): number {
    const generation = (this.socialGeneration.get(id) ?? 0) + 1;
    this.socialGeneration.set(id, generation);
    return generation;
  }

  private nextSocialRequest(id: string): number {
    const request = (this.socialRequests.get(id) ?? 0) + 1;
    this.socialRequests.set(id, request);
    return request;
  }

  private async waitForMutations(id: string): Promise<void> {
    while (this.mutationQueues.has(id)) await this.mutationQueues.get(id);
  }

  async social(id: string): Promise<void> {
    // A refresh requested during a mutation must read its committed state.
    const epoch = this.epoch;
    await this.waitForMutations(id);
    if (!this.valid(id, epoch)) return;
    const generation = this.socialGeneration.get(id) ?? 0;
    const request = this.nextSocialRequest(id);
    const [likes, index] = await Promise.all([this.api.likes(id), this.api.commentIndex(id)]);
    if (this.valid(id, epoch) && generation === (this.socialGeneration.get(id) ?? 0)
      && request === this.socialRequests.get(id)) {
      this.likes[id] = likes;
      this.indexes[id] = index;
    }
  }

  private mutate<T>(id: string, request: () => Promise<T>, publish: (value: T) => void): Promise<T> {
    const epoch = this.epoch;
    const previous = this.mutationQueues.get(id) ?? Promise.resolve();
    const next = previous.then(async () => {
      if (!this.session || !this.valid(id, epoch)) throw new Error('Sign in to continue.');
      const generation = this.bumpSocial(id);
      const value = await request();
      if (this.valid(id, epoch) && generation === this.socialGeneration.get(id)) publish(value);
      return value;
    });
    const settled = next.then(() => {}, () => {});
    this.mutationQueues.set(id, settled);
    void settled.then(() => {
      if (this.mutationQueues.get(id) === settled) this.mutationQueues.delete(id);
    });
    return next;
  }

  toggleLike(id: string, postId: string): Promise<void> {
    return this.mutate(id, () => this.api.toggleLike(id, postId),
      (likes) => { this.likes[id] = likes; }).then(() => {});
  }

  async comments(id: string, postId: string): Promise<PostComment[]> {
    const epoch = this.epoch;
    await this.waitForMutations(id);
    if (!this.session || !this.valid(id, epoch)) throw new Error('Sign in to continue.');
    const generation = this.socialGeneration.get(id) ?? 0;
    const comments = await this.api.comments(id, postId);
    if (this.valid(id, epoch) && generation === (this.socialGeneration.get(id) ?? 0)) {
      this.bumpSocial(id);
      this.afterThread(id, postId, comments);
    }
    return comments;
  }

  addComment(id: string, postId: string, text: string): Promise<PostComment[]> {
    return this.mutate(id, () => this.api.addComment(id, postId, text),
      (comments) => this.afterThread(id, postId, comments));
  }

  deleteComment(id: string, postId: string, commentId: string): Promise<PostComment[]> {
    return this.mutate(id, () => this.api.deleteComment(id, postId, commentId),
      (comments) => this.afterThread(id, postId, comments));
  }

  private afterThread(id: string, postId: string, comments: PostComment[]): void {
    const current = this.indexes[id] ?? { counts: {}, mine: [] };
    this.indexes[id] = {
      counts: { ...current.counts, [postId]: comments.length },
      mine: [...current.mine.filter((value) => value !== postId),
        ...(comments.some((comment) => comment.mine) ? [postId] : [])],
    };
  }

  async loadLikes(): Promise<void> {
    const epoch = this.epoch;
    await Promise.all(this.books.map(async (book) => {
      await this.waitForMutations(book.id);
      if (!this.valid(book.id, epoch)) return;
      const generation = this.socialGeneration.get(book.id) ?? 0;
      const request = this.nextSocialRequest(book.id);
      const ids = await this.api.likes(book.id);
      if (!this.valid(book.id, epoch)) return;
      if (generation === (this.socialGeneration.get(book.id) ?? 0)
        && request === this.socialRequests.get(book.id)) this.likes[book.id] = ids;
      if (ids.length) {
        const [, index] = await Promise.all([this.feed(book.id), this.api.commentIndex(book.id)]);
        if (this.valid(book.id, epoch) && generation === (this.socialGeneration.get(book.id) ?? 0)
          && request === this.socialRequests.get(book.id)) {
          this.indexes[book.id] = index;
        }
      }
    }));
  }

  likedPosts(): PostHit[] {
    return this.books.flatMap((book) => {
      const ids = new Set(this.likes[book.id] ?? []);
      return (this.feeds[book.id] ?? []).filter((post) => ids.has(post.id))
        .map((post) => ({ book, post }));
    });
  }

  async profile(username: string): Promise<ProfileReply[]> {
    const epoch = this.epoch;
    const remote = await this.api.profile(username);
    if (epoch !== this.epoch) return [];
    const groups = new Map<string, typeof remote.replies>();
    for (const row of remote.replies) {
      const group = groups.get(row.bookId) ?? [];
      group.push(row);
      groups.set(row.bookId, group);
    }
    const replies = await Promise.all([...groups].map(async ([id, rows]) => {
      const book = this.books.find((row) => row.id === id);
      if (!book) return [];
      await Promise.all([this.feed(id), this.social(id)]);
      if (!this.valid(id, epoch)) return [];
      const posts = this.postMaps.get(id);
      return rows.flatMap((row) => {
        const post = posts?.get(row.postId);
        return post ? [{ book, post, comment: row.comment }] : [];
      });
    }));
    return replies.flat().filter((reply) => this.valid(reply.book.id, epoch))
      .sort((a, b) => b.comment.createdAt - a.comment.createdAt);
  }

  async upload(file: File, sample: boolean, progress: (percent: number) => void): Promise<BookRecord> {
    const epoch = this.epoch;
    const book = await this.api.upload(file, sample, progress);
    if (this.valid(book.id, epoch)) {
      this.replaceBook(book);
      void this.loadCover(book.id).catch(() => {});
    }
    return book;
  }

  async setShared(id: string, shared: boolean): Promise<void> {
    const epoch = this.epoch;
    const book = this.books.find((row) => row.id === id);
    if (!book || this.removed.has(id)) return;
    if (!book.mine) {
      throw new Error('Only the person who added this book can change sharing.');
    }
    if (book.shared === shared && !this.shareTails.has(id)) return;
    if (!this.shareTails.has(id)) this.confirmedShare.set(id, book.shared);
    if (book.shared !== shared) this.replaceBook({ ...book, shared });

    const previous = this.shareTails.get(id) ?? Promise.resolve();
    const task = previous.catch(() => {}).then(async () => {
      try {
        const next = await this.api.setShared(id, shared);
        if (!this.valid(id, epoch)) return;
        this.confirmedShare.set(id, next.shared);
        const current = this.books.find((row) => row.id === id);
        if (current && current.shared === shared) {
          this.replaceBook({
            ...current,
            shared: next.shared,
            mine: next.mine,
            ownerUsername: next.ownerUsername,
          });
        }
      } catch (error) {
        if (!this.valid(id, epoch)) return;
        const current = this.books.find((row) => row.id === id);
        // A newer toggle already shows the user's latest choice.
        if (!current || current.shared !== shared) return;
        const confirmed = this.confirmedShare.get(id);
        if (confirmed !== undefined) this.replaceBook({ ...current, shared: confirmed });
        throw error;
      }
    });
    const settled = task.then(() => {}, () => {});
    this.shareTails.set(id, settled);
    void settled.then(() => {
      if (this.shareTails.get(id) === settled) {
        this.shareTails.delete(id);
        this.confirmedShare.delete(id);
      }
    });
    return task;
  }

  async remove(book: BookRecord): Promise<void> {
    const epoch = this.epoch;
    await this.api.remove(book.id);
    if (epoch !== this.epoch) return;
    this.removed.add(book.id);
    this.progress.remove(book.id);
    this.pendingFeeds.delete(book.id);
    this.pendingCovers.delete(book.id);
    this.postMaps.delete(book.id);
    this.bumpSocial(book.id);
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

  saveProgress(id: string, index: number): void {
    const book = this.books.find((book) => book.id === id);
    if (!book) return;
    if (book.progressIndex === index && !this.progress.has(id)) return;
    if (book.progressIndex !== index) this.replaceBook({ ...book, progressIndex: index });
    // Even an unchanged local index must retry a previous failed save.
    this.progress.save(id, index);
  }

  flushProgress(onlyId?: string, background = false): void {
    this.progress.flush(onlyId, background);
  }
}

export const store = new HostedStore();
