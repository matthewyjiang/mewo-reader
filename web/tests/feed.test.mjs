import assert from 'node:assert/strict';
import { test } from 'node:test';
import {
  appendFeed, beginFeedRefresh, failFeed, feedMoreAllowed, finishFeedRefresh, phaseFromFeedCache,
  showFeedEmpty, startFeedMore,
} from '../src/lib/feed.ts';

const page = (ids, nextCursor = null) => ({
  items: ids.map((id) => ({
    type: 'repost',
    actor: { username: 'ada' },
    book: { id: 'b' },
    post: { id },
    at: 1,
  })),
  nextCursor,
});

test('an empty answer is the only empty state', () => {
  assert.equal(showFeedEmpty({ status: 'loading' }), false);
  assert.equal(showFeedEmpty({ status: 'failed', error: 'offline', page: null }), false);
  assert.equal(showFeedEmpty(finishFeedRefresh(page([]))), true);
  assert.equal(showFeedEmpty(finishFeedRefresh(page(['1']))), false);
});

test('a cached page shows immediately and a failed refresh keeps it', () => {
  const cached = phaseFromFeedCache(page(['1']));
  assert.equal(cached.status, 'loaded');
  assert.equal(cached.refreshing, true);
  assert.equal(showFeedEmpty(cached), false);
  const refreshing = beginFeedRefresh(cached);
  const failed = failFeed(refreshing, 'offline');
  assert.equal(failed.status, 'failed');
  assert.equal(failed.page.items.length, 1);
  assert.equal(showFeedEmpty(failed), false);
  assert.equal(phaseFromFeedCache(null).status, 'loading');
});

test('the next page waits until a refresh has finished', () => {
  const cached = phaseFromFeedCache(page(['1'], 'c'));
  assert.equal(feedMoreAllowed(cached), false);
  assert.equal(feedMoreAllowed(beginFeedRefresh(cached)), false);
  const fresh = finishFeedRefresh(page(['1'], 'c'));
  assert.equal(feedMoreAllowed(fresh), true);
  assert.equal(feedMoreAllowed(startFeedMore(fresh)), false);
  assert.equal(feedMoreAllowed(finishFeedRefresh(page(['1']))), false);
});

test('the next cursor appends and a failed next page keeps the rows', () => {
  const first = finishFeedRefresh(page(['1'], 'c'));
  const more = startFeedMore(first);
  assert.equal(more.more, true);
  const both = appendFeed(more, page(['2'], 'd'));
  assert.deepEqual(both.page.items.map((item) => item.post.id), ['1', '2']);
  const failed = failFeed(startFeedMore(both), 'offline');
  assert.equal(failed.status, 'failed');
  assert.equal(failed.page.items.length, 2);
});
