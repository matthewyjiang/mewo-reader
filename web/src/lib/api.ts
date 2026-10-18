import type {
  AuthResponse, BookRecord, CommentIndex, FeedPost, HomeFeedPage, PostComment,
  ProfileBooks, ProfilePostsPage, ProfileRecord, ProfileRepostsPage, RepostState,
  SearchKind, SearchPage,
} from './models';

export class ApiError extends Error {
  constructor(public readonly status: number, message: string) {
    super(message);
    this.name = 'ApiError';
  }
}

function errorMessage(status: number, body: string): string {
  try {
    const value: unknown = JSON.parse(body);
    if (value && typeof value === 'object'
      && 'error' in value && typeof value.error === 'string') return value.error;
  } catch {
    // A proxy may return non-JSON errors.
  }
  return `Server returned ${status}. Try again.`;
}

/** Same-origin bearer client. Covers and uploads also expire sessions on 401. */
export class HostedClient {
  constructor(private token: () => string | undefined, private expire: () => void) {}

  private fail(status: number, body: string, requestToken: string | undefined): never {
    if (status === 401 && requestToken === this.token()) this.expire();
    throw new ApiError(status, errorMessage(status, body));
  }

  private async response(path: string, init: RequestInit = {}): Promise<Response> {
    const token = this.token();
    const headers = new Headers(init.headers);
    if (token) headers.set('Authorization', `Bearer ${token}`);
    let response: Response;
    try {
      response = await fetch(`/v1${path}`, { ...init, headers });
    } catch (error) {
      if (error instanceof DOMException && error.name === 'AbortError') throw error;
      throw new ApiError(0, 'Could not reach the server. Check your connection and try again.');
    }
    if (!response.ok) this.fail(response.status, await response.text(), token);
    return response;
  }

  private async json<T>(path: string, method = 'GET', body?: unknown, signal?: AbortSignal): Promise<T> {
    const init: RequestInit = { method, signal };
    if (body !== undefined) {
      init.headers = { 'Content-Type': 'application/json' };
      init.body = JSON.stringify(body);
    }
    const response = await this.response(path, init);
    try {
      return await response.json() as T;
    } catch {
      throw new ApiError(response.status, 'Could not read the server response.');
    }
  }

  auth(mode: 'login' | 'register', username: string, password: string): Promise<AuthResponse> {
    return this.json(`/auth/${mode}`, 'POST', { username, password });
  }

  async logout(): Promise<void> {
    await this.response('/auth/logout', { method: 'POST' });
  }

  me(): Promise<{ username: string }> {
    return this.json('/me');
  }

  async library(): Promise<BookRecord[]> {
    return (await this.json<{ books: BookRecord[] }>('/library')).books;
  }

  book(id: string): Promise<BookRecord> {
    return this.json(this.bookPath(id));
  }

  async posts(id: string): Promise<FeedPost[]> {
    const result = await this.json<{ posts: FeedPost[] }>(this.bookPath(id, '/posts'));
    return result.posts;
  }

  async cover(id: string): Promise<Blob | null> {
    try {
      return await (await this.response(this.bookPath(id, '/cover'))).blob();
    } catch (error) {
      if (error instanceof ApiError && error.status === 404) return null;
      throw error;
    }
  }

  async remove(id: string): Promise<void> {
    await this.response(this.bookPath(id), { method: 'DELETE' });
  }

  /** Owner-only. Other accounts that can see the book get 403. */
  setShared(id: string, shared: boolean): Promise<BookRecord> {
    return this.json(this.bookPath(id), 'PATCH', { shared });
  }

  progress(id: string, progressIndex: number, keepalive = false): Promise<Response> {
    return this.response(this.bookPath(id), {
      method: 'PATCH',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ progressIndex }),
      keepalive,
    });
  }

  async likes(id: string): Promise<string[]> {
    return (await this.json<{ ids: string[] }>(this.bookPath(id, '/likes'))).ids;
  }

  async toggleLike(id: string, postId: string): Promise<string[]> {
    const path = this.bookPath(id, `/likes/${encodeURIComponent(postId)}`);
    return (await this.json<{ ids: string[] }>(path, 'POST')).ids;
  }

  commentIndex(id: string): Promise<CommentIndex> {
    return this.json(this.bookPath(id, '/comments'));
  }

  async comments(id: string, postId: string): Promise<PostComment[]> {
    const result = await this.json<{ comments: PostComment[] }>(this.threadPath(id, postId));
    return result.comments;
  }

  async addComment(id: string, postId: string, text: string): Promise<PostComment[]> {
    const result = await this.json<{ comments: PostComment[] }>(
      this.threadPath(id, postId), 'POST', { text },
    );
    return result.comments;
  }

  async deleteComment(id: string, postId: string, commentId: string): Promise<PostComment[]> {
    const path = `${this.threadPath(id, postId)}/${encodeURIComponent(commentId)}`;
    return (await this.json<{ comments: PostComment[] }>(path, 'DELETE')).comments;
  }

  search(q: string, type: SearchKind, cursor?: string | null, signal?: AbortSignal): Promise<SearchPage> {
    const params = new URLSearchParams({ q, type });
    if (cursor) params.set('cursor', cursor);
    return this.json(`/search?${params}`, 'GET', undefined, signal);
  }

  user(username: string, signal?: AbortSignal): Promise<ProfileRecord> {
    return this.json(`/users/${encodeURIComponent(username)}`, 'GET', undefined, signal);
  }

  userPosts(username: string, cursor?: string | null, signal?: AbortSignal): Promise<ProfilePostsPage> {
    return this.json(this.userPath(username, '/posts', cursor), 'GET', undefined, signal);
  }

  userReposts(
    username: string,
    cursor?: string | null,
    signal?: AbortSignal,
  ): Promise<ProfileRepostsPage> {
    return this.json(this.userPath(username, '/reposts', cursor), 'GET', undefined, signal);
  }

  userBooks(username: string, signal?: AbortSignal): Promise<ProfileBooks> {
    return this.json(this.userPath(username, '/books'), 'GET', undefined, signal);
  }

  homeFeed(cursor?: string | null, signal?: AbortSignal): Promise<HomeFeedPage> {
    const params = new URLSearchParams();
    if (cursor) params.set('cursor', cursor);
    const query = params.toString();
    return this.json(`/feed${query ? `?${query}` : ''}`, 'GET', undefined, signal);
  }

  setRepost(id: string, postId: string, reposted: boolean): Promise<RepostState> {
    const path = this.bookPath(id, `/posts/${encodeURIComponent(postId)}/repost`);
    return this.json(path, reposted ? 'PUT' : 'DELETE');
  }

  private userPath(username: string, suffix: string, cursor?: string | null): string {
    const params = new URLSearchParams();
    if (cursor) params.set('cursor', cursor);
    const query = params.toString();
    return `/users/${encodeURIComponent(username)}${suffix}${query ? `?${query}` : ''}`;
  }

  private bookPath(id: string, suffix = ''): string {
    return `/books/${encodeURIComponent(id)}${suffix}`;
  }

  private threadPath(id: string, postId: string): string {
    return this.bookPath(id, `/comments/${encodeURIComponent(postId)}`);
  }

  // XHR is used only here because fetch does not expose upload progress.
  upload(
    epub: File,
    isSample: boolean,
    onProgress: (percent: number) => void,
  ): Promise<BookRecord> {
    const token = this.token();
    return new Promise((resolve, reject) => {
      const xhr = new XMLHttpRequest();
      xhr.open('POST', '/v1/books');
      if (token) xhr.setRequestHeader('Authorization', `Bearer ${token}`);
      xhr.upload.onprogress = (event) => {
        if (event.lengthComputable) onProgress(Math.round(event.loaded * 100 / event.total));
      };
      xhr.onerror = () => {
        reject(new ApiError(0, 'Upload failed. Check your connection and try again.'));
      };
      xhr.onabort = () => reject(new ApiError(0, 'Upload cancelled.'));
      xhr.onload = () => {
        try {
          if (xhr.status < 200 || xhr.status >= 300) {
            this.fail(xhr.status, xhr.responseText, token);
          }
          resolve(JSON.parse(xhr.responseText) as BookRecord);
        } catch (error) {
          reject(error instanceof ApiError
            ? error : new ApiError(xhr.status, 'Could not read the server response.'));
        }
      };
      const body = new FormData();
      body.append('epub', epub);
      if (isSample) body.append('isSample', 'true');
      xhr.send(body);
    });
  }
}
