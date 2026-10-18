import type { CommentIndex, RepostState } from './models';

export function repostCount(index: CommentIndex | undefined, postId: string): number {
  return index?.reposts?.[postId] ?? 0;
}

export function hasReposted(index: CommentIndex | undefined, postId: string): boolean {
  return index?.reposted?.includes(postId) ?? false;
}

function withCount(reposts: Record<string, number>, postId: string, count: number): Record<string, number> {
  const next = { ...reposts };
  if (count <= 0) delete next[postId];
  else next[postId] = count;
  return next;
}

function withFlag(ids: string[], postId: string, on: boolean): string[] {
  const rest = ids.filter((id) => id !== postId);
  return on ? [...rest, postId] : rest;
}

/** One post flips. Other posts on the same book stay as they are. */
export function applyRepost(index: CommentIndex, postId: string, reposted: boolean): CommentIndex {
  const already = hasReposted(index, postId);
  if (already === reposted) return index;
  const count = repostCount(index, postId) + (reposted ? 1 : -1);
  return {
    ...index,
    reposts: withCount(index.reposts ?? {}, postId, count),
    reposted: withFlag(index.reposted ?? [], postId, reposted),
  };
}

/** The server's count replaces the optimistic one for this post only. */
export function applyServerRepost(
  index: CommentIndex,
  postId: string,
  state: RepostState,
): CommentIndex {
  return {
    ...index,
    reposts: withCount(index.reposts ?? {}, postId, state.reposts),
    reposted: withFlag(index.reposted ?? [], postId, state.reposted),
  };
}

/** Put this post back. A newer toggle on the same post is left alone by the caller. */
export function rollbackRepost(
  current: CommentIndex,
  before: CommentIndex,
  postId: string,
): CommentIndex {
  return {
    ...current,
    reposts: withCount(current.reposts ?? {}, postId, repostCount(before, postId)),
    reposted: withFlag(current.reposted ?? [], postId, hasReposted(before, postId)),
  };
}

export function sameRepost(left: CommentIndex, right: CommentIndex, postId: string): boolean {
  return hasReposted(left, postId) === hasReposted(right, postId)
    && repostCount(left, postId) === repostCount(right, postId);
}

/**
 * Show the flipped count, then keep the server's answer. A failure puts the
 * post back unless a newer toggle already replaced it.
 */
export async function commitRepost(
  before: CommentIndex,
  postId: string,
  request: (reposted: boolean) => Promise<RepostState>,
  read: () => CommentIndex,
  write: (index: CommentIndex) => void,
): Promise<void> {
  const reposted = !hasReposted(before, postId);
  const optimistic = applyRepost(before, postId, reposted);
  write(optimistic);
  try {
    const state = await request(reposted);
    const now = read();
    if (sameRepost(now, optimistic, postId)) write(applyServerRepost(now, postId, state));
  } catch (error) {
    const now = read();
    if (sameRepost(now, optimistic, postId)) write(rollbackRepost(now, before, postId));
    throw error;
  }
}
