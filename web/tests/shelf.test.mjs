import assert from 'node:assert/strict';
import { test } from 'node:test';
import { MemoryPosts, loadCachedPosts, postKey, postPrefix } from '../src/lib/posts.ts';
import { pullOffset, PULL_THRESHOLD, REFRESH_GAP, refreshGap, shouldRefresh } from '../src/lib/pull.ts';
import {
  ShelfRefresh, beginRefresh, clearShelf, failRefresh, finishRefresh, phaseFromCache,
  readShelf, shelfBooks, shelfKey, showEmpty, writeShelf,
} from '../src/lib/shelf.ts';

const server = 'https://mewo.example';

function book(id, extra = {}) {
  return {
    id,
    title: extra.title ?? id,
    author: 'Austen',
    handle: 'austen',
    importedAt: extra.importedAt ?? 1,
    postCount: extra.postCount ?? 12,
    progressIndex: extra.progressIndex ?? 3,
    isSample: false,
    mine: extra.mine ?? true,
    shared: extra.shared ?? false,
    ownerUsername: extra.ownerUsername ?? 'ada',
  };
}

function memoryStore() {
  const rows = new Map();
  return {
    getItem: (key) => rows.get(key) ?? null,
    setItem: (key, value) => rows.set(key, value),
    removeItem: (key) => rows.delete(key),
  };
}

test('empty copy waits for a successful fetch', () => {
  const loading = { status: 'loading' };
  assert.equal(showEmpty(loading), false);
  assert.deepEqual(shelfBooks(loading), []);

  const none = finishRefresh([]);
  assert.equal(showEmpty(none), true);
  assert.equal(none.refreshing, false);

  const cached = phaseFromCache([book('a')]);
  assert.equal(showEmpty(cached), false);
  assert.equal(cached.refreshing, true);
  assert.equal(showEmpty(beginRefresh(cached)), false);
});

test('refresh keeps the shelf when the server fails', () => {
  const cached = phaseFromCache([book('a', { progressIndex: 4, shared: true, mine: false })]);
  const started = beginRefresh(cached);
  assert.equal(started.status, 'loaded');
  assert.equal(started.refreshing, true);
  assert.equal(started.books[0].progressIndex, 4);
  assert.equal(started.books[0].shared, true);
  assert.equal(started.books[0].ownerUsername, 'ada');

  const failed = failRefresh(started, 'offline');
  assert.equal(failed.status, 'failed');
  assert.equal(failed.books.length, 1);
  assert.equal(showEmpty(failed), false);

  const first = failRefresh({ status: 'loading' }, 'offline');
  assert.equal(first.books, null);
  assert.equal(showEmpty(first), false);
  assert.equal(beginRefresh(first).status, 'loading');
});

test('a second refresh joins the first, then the next one calls again', async () => {
  let calls = 0;
  let release;
  const gate = new Promise((resolve) => { release = resolve; });
  /** @type {import('../src/lib/shelf.ts').LibraryPhase[]} */
  const seen = [];
  const shelf = new ShelfRefresh({ status: 'loading' }, (phase) => seen.push(phase));
  shelf.prime([book('cached')]);
  const first = shelf.refresh(async () => {
    calls += 1;
    await gate;
    return [book('fresh', { importedAt: 2 })];
  });
  const second = shelf.refresh(async () => {
    calls += 1;
    return [book('ignored')];
  });
  assert.equal(shelf.phase.status, 'loaded');
  assert.equal(shelf.phase.refreshing, true);
  assert.equal(shelf.phase.books[0].id, 'cached');
  release();
  await first;
  await second;
  assert.equal(calls, 1);
  assert.equal(shelf.phase.status, 'loaded');
  assert.equal(shelf.phase.refreshing, false);
  assert.equal(shelf.phase.books[0].id, 'fresh');

  await shelf.refresh(async () => {
    calls += 1;
    throw new Error('offline');
  });
  assert.equal(calls, 2);
  assert.equal(shelf.phase.status, 'failed');
  assert.equal(shelf.phase.books[0].id, 'fresh');
  assert.equal(shelf.phase.error, 'offline');
});

test('shelf cache is per server and username', () => {
  const store = memoryStore();
  const ada = [book('a', { ownerUsername: 'ada', mine: true, shared: false, progressIndex: 8 })];
  const matt = [book('b', { ownerUsername: 'matt', mine: false, shared: true, progressIndex: 1 })];
  writeShelf(server, 'ada', ada, store);
  writeShelf(server, 'matt', matt, store);
  writeShelf('http://other.example', 'ada', [book('c')], store);
  assert.notEqual(shelfKey(server, 'ada'), shelfKey(server, 'matt'));
  assert.notEqual(shelfKey(server, 'ada'), shelfKey('http://other.example', 'ada'));
  assert.equal(readShelf(server, 'ada', store)[0].id, 'a');
  assert.equal(readShelf(server, 'ada', store)[0].progressIndex, 8);
  assert.equal(readShelf(server, 'ada', store)[0].mine, true);
  assert.equal(readShelf(server, 'matt', store)[0].shared, true);
  assert.equal(readShelf(server, 'matt', store)[0].ownerUsername, 'matt');
  assert.equal(readShelf('http://other.example', 'ada', store)[0].id, 'c');
  clearShelf(server, 'ada', store);
  assert.equal(readShelf(server, 'ada', store), null);
  assert.equal(readShelf(server, 'matt', store)[0].id, 'b');
  store.setItem(shelfKey(server, 'ada'), '{');
  assert.equal(readShelf(server, 'ada', store), null);
});

test('posts are reused per account and not downloaded again', async () => {
  const cache = new MemoryPosts();
  let adaFetches = 0;
  let mattFetches = 0;
  const adaKey = postKey(server, 'ada', 'book');
  const mattKey = postKey(server, 'matt', 'book');
  const first = await loadCachedPosts(adaKey, cache, async () => {
    adaFetches += 1;
    return [{ id: 'p', kind: 'Body', text: 'Hello', chapter: '1' }];
  });
  const second = await loadCachedPosts(adaKey, cache, async () => {
    adaFetches += 1;
    return [{ id: 'other', kind: 'Body', text: 'nope', chapter: '1' }];
  });
  assert.equal(adaFetches, 1);
  assert.equal(second[0].text, first[0].text);
  const other = await loadCachedPosts(mattKey, cache, async () => {
    mattFetches += 1;
    return [{ id: 'm', kind: 'Heading', text: 'Matt', chapter: '2' }];
  });
  assert.equal(mattFetches, 1);
  assert.equal(other[0].text, 'Matt');
  assert.equal((await cache.read(adaKey))[0].text, 'Hello');
  await cache.clearPrefix(postPrefix(server, 'ada'));
  assert.equal(await cache.read(adaKey), null);
  assert.equal((await cache.read(mattKey))[0].id, 'm');
});

test('pull arms a refresh at the threshold and skips the drag when motion is reduced', () => {
  assert.equal(shouldRefresh(pullOffset(0, false)), false);
  assert.equal(shouldRefresh(pullOffset(40, false)), false);
  assert.equal(pullOffset(40, true), 0);
  const armed = pullOffset(160, false);
  assert.equal(shouldRefresh(armed), true);
  assert.equal(pullOffset(PULL_THRESHOLD, true), PULL_THRESHOLD);
  assert.equal(shouldRefresh(pullOffset(PULL_THRESHOLD, true)), true);
  assert.ok(pullOffset(400, false) <= 80);

  assert.equal(refreshGap(0, false, false), 0);
  assert.equal(refreshGap(0, true, false), REFRESH_GAP);
  assert.equal(refreshGap(72, false, false), 72);
  assert.equal(refreshGap(72, true, false), 72);
  assert.equal(refreshGap(72, false, true), 0);
  assert.equal(refreshGap(0, true, true), REFRESH_GAP);
});
