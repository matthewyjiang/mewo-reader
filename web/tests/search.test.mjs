import assert from 'node:assert/strict';
import { describe, test } from 'node:test';
import {
  BROWSE_REVALIDATE_MS, CATALOG_DEBOUNCE_MS, emptyKind, isBrowse, SearchModel,
  SEARCH_DEBOUNCE_MS, searchMemo, trendingNote,
} from '../src/lib/search.ts';

function deferred() {
  let resolve;
  let reject;
  const promise = new Promise((res, rej) => { resolve = res; reject = rej; });
  return { promise, resolve, reject };
}

function harness(session = 'token', kind = 'accounts', now = () => 0, keepMemo = false) {
  if (session && !keepMemo) searchMemo(session).clear();
  const timers = [];
  const calls = [];
  let pending = deferred();
  const model = new SearchModel(
    (request) => {
      calls.push({ query: request.query, kind: request.kind, cursor: request.cursor, session: request.session });
      request.signal.addEventListener('abort', () => pending.reject(new DOMException('aborted', 'AbortError')));
      return pending.promise;
    },
    () => {},
    (fn, ms) => {
      const id = { fn, ms };
      timers.push(id);
      return id;
    },
    (id) => {
      const index = timers.indexOf(id);
      if (index >= 0) timers.splice(index, 1);
    },
    session,
    kind,
    now,
  );
  return {
    model,
    calls,
    timers,
    replace() { pending = deferred(); },
    fire(ms) {
      const id = timers.shift();
      assert.ok(id, 'expected a timer');
      assert.equal(id.ms, ms === undefined
        ? (kind === 'catalog' ? CATALOG_DEBOUNCE_MS : SEARCH_DEBOUNCE_MS)
        : ms);
      id.fn();
    },
    settle(page) {
      pending.resolve(page);
      return pending.promise.catch(() => {});
    },
    fail(error) { pending.reject(error); return pending.promise.catch(() => {}); },
  };
}

const emptyAccounts = { type: 'accounts', items: [], nextCursor: null };
const ada = { type: 'accounts', items: [{ username: 'ada', mine: false }], nextCursor: null };
const books = {
  type: 'books',
  items: [{ id: 'b1', title: 'Emma' }],
  nextCursor: null,
};

describe('SearchModel', { concurrency: false }, () => {
  test('no matches waits for a successful empty page', async () => {
    const { model, fire, settle } = harness();
    model.setQuery('ada');
    assert.equal(emptyKind(model.state), 'none');
    assert.equal(model.state.phase.status, 'loading');
    fire();
    assert.equal(emptyKind(model.state), 'none');
    await settle(emptyAccounts);
    assert.equal(emptyKind(model.state), 'noMatches');
  });

  test('a keystroke inside the debounce window cancels the older request', async () => {
    const { model, calls, timers, fire, replace } = harness();
    model.setQuery('a');
    assert.equal(calls.length, 0);
    assert.equal(timers.length, 1);
    model.setQuery('ab');
    assert.equal(timers.length, 1);
    fire();
    assert.equal(calls.length, 1);
    assert.equal(calls[0].query, 'ab');
    replace();
  });

  test('a stale page is dropped after sign-out and after a newer query', async () => {
    const { model, fire, settle, replace } = harness();
    model.setQuery('ada');
    fire();
    model.setSession(null);
    await settle(ada);
    assert.equal(model.state.phase.status, 'idle');
    assert.equal(model.state.session, null);

    model.setSession('token');
    model.setQuery('');
    replace();
    model.setQuery('ada');
    fire();
    model.setQuery('bea');
    await settle(ada);
    assert.equal(model.state.phase.status, 'loading');
    assert.equal(model.state.query, 'bea');
    assert.equal(emptyKind(model.state), 'none');
  });

  test('an error keeps the previous page and retry asks again', async () => {
    const { model, fire, settle, fail, replace, calls } = harness();
    model.setQuery('ada');
    fire();
    await settle(ada);
    replace();
    model.setQuery('bea');
    fire();
    await fail(new Error('offline'));
    assert.equal(model.state.phase.status, 'failed');
    assert.equal(model.state.phase.previous, ada);
    assert.equal(emptyKind(model.state), 'none');
    replace();
    model.retry();
    assert.equal(calls.at(-1).query, 'bea');
    await settle({ type: 'accounts', items: [{ username: 'bea', mine: false }], nextCursor: null });
    assert.equal(model.state.phase.status, 'loaded');
    assert.equal(model.state.phase.page.items[0].username, 'bea');
  });

  test('catalog search waits 300ms and the other tabs stay at 250ms', () => {
    const accounts = harness();
    accounts.model.setQuery('ada');
    assert.equal(accounts.timers[0].ms, SEARCH_DEBOUNCE_MS);
    const catalog = harness('token', 'catalog');
    catalog.model.setQuery('dune');
    assert.equal(catalog.timers[0].ms, CATALOG_DEBOUNCE_MS);
    assert.equal(CATALOG_DEBOUNCE_MS, 300);
  });

  test('a blank query with a session fetches the browse page', async () => {
    const { model, timers, calls, fire, settle } = harness();
    assert.equal(model.state.phase.status, 'loading');
    assert.equal(isBrowse(model.state), true);
    assert.equal(timers[0].ms, 0);
    fire(0);
    assert.equal(calls[0].query, '');
    assert.equal(calls[0].kind, 'accounts');
    assert.equal(calls[0].cursor, null);
    await settle(emptyAccounts);
    assert.equal(model.state.phase.status, 'loaded');
    assert.equal(emptyKind(model.state), 'nothingYet');
  });

  test('no session is idle', () => {
    const { model, timers } = harness(null);
    assert.equal(model.state.phase.status, 'idle');
    assert.equal(timers.length, 0);
    model.setQuery('ada');
    model.setKind('books');
    model.loadMore();
    model.retry();
    assert.equal(model.state.phase.status, 'idle');
    assert.equal(timers.length, 0);
  });

  test('switching tabs shows the cached page and revalidates browse after 60s', async () => {
    let now = 0;
    const clock = () => now;
    const { model, fire, settle, calls, timers, fail, replace } = harness('memo', 'books', clock);
    fire(0);
    await settle(books);
    replace();
    model.setKind('accounts');
    assert.equal(model.state.phase.status, 'loading');
    fire(0);
    await settle(ada);
    model.setKind('books');
    assert.equal(model.state.phase.status, 'loaded');
    assert.equal(model.state.phase.page, books);
    assert.equal(timers.length, 0);

    now = BROWSE_REVALIDATE_MS + 1;
    model.setKind('accounts');
    assert.equal(model.state.phase.status, 'loaded');
    assert.equal(model.state.phase.page, ada);
    assert.equal(timers.length, 1);
    replace();
    const before = calls.length;
    fire(0);
    assert.equal(calls.length, before + 1);
    assert.equal(calls.at(-1).query, '');
    await fail(new Error('offline'));
    assert.equal(model.state.phase.status, 'loaded');
    assert.equal(model.state.phase.page, ada);
  });

  test('a typed query is not revalidated from the memo', async () => {
    let now = 0;
    const { model, fire, settle, timers } = harness('typed', 'books', () => now);
    model.setQuery('dune');
    fire();
    await settle(books);
    now = BROWSE_REVALIDATE_MS * 10;
    model.setKind('accounts');
    fire();
    model.setKind('books');
    assert.equal(model.state.phase.status, 'loaded');
    assert.equal(model.state.phase.page, books);
    assert.equal(timers.length, 0);
  });

  test('clearing the query shows the browse memo immediately', async () => {
    const { model, fire, settle, timers } = harness('clear');
    fire(0);
    await settle(ada);
    model.setQuery('bea');
    fire();
    await settle({ type: 'accounts', items: [{ username: 'bea', mine: false }], nextCursor: null });
    model.setQuery('');
    assert.equal(model.state.phase.status, 'loaded');
    assert.equal(model.state.phase.page, ada);
    assert.equal(timers.length, 0);
  });

  test('a remount with the same token starts loaded', async () => {
    const first = harness('remount', 'books');
    first.fire(0);
    await first.settle(books);
    const second = harness('remount', 'books', () => 0, true);
    assert.equal(second.model.state.phase.status, 'loaded');
    assert.equal(second.model.state.phase.page, books);
    assert.equal(second.timers.length, 0);
  });

  test('a token change clears the memo', async () => {
    const { model, fire, settle } = harness('drop');
    fire(0);
    await settle(ada);
    const memo = searchMemo('drop');
    assert.equal(memo.size, 1);
    model.setSession('other');
    assert.equal(memo.size, 0);
    assert.equal(searchMemo('drop').size, 0);
    assert.notEqual(model.state.phase.status, 'idle');
  });

  test('the memo keeps the eight newest pages', async () => {
    const { model, fire, settle, replace } = harness('lru');
    fire(0);
    await settle(ada);
    for (let i = 0; i < 8; i += 1) {
      replace();
      model.setQuery(`q${i}`);
      fire();
      await settle({ type: 'accounts', items: [{ username: `u${i}`, mine: false }], nextCursor: null });
    }
    assert.equal(searchMemo('lru').size, 8);
    model.setQuery('');
    assert.equal(model.state.phase.status, 'loading');
  });

  test('a catalog request is written into the memo a remount reads', async () => {
    const { model, fire, settle } = harness('note', 'catalog');
    const dune = { workKey: '/works/OL893415W', title: 'Dune', request: null, onServer: null };
    fire(0);
    await settle({ type: 'catalog', items: [dune], nextCursor: '2' });
    const requested = {
      id: 'req-1', workKey: dune.workKey, title: 'Dune', state: 'requested', progress: 0, error: null,
    };
    model.noteRequest(dune.workKey, requested);
    assert.equal(model.state.phase.page.items[0].request, requested);
    const second = harness('note', 'catalog', () => 0, true);
    assert.equal(second.model.state.phase.page.items[0].request.state, 'requested');
  });

  test('browse catalog load more asks for the next page', async () => {
    const { model, fire, settle, calls, replace } = harness('pages', 'catalog');
    fire(0);
    await settle({
      type: 'catalog',
      items: [{ workKey: '/works/OL1W', title: 'Dune' }],
      nextCursor: '2',
    });
    replace();
    model.loadMore();
    assert.equal(calls.at(-1).query, '');
    assert.equal(calls.at(-1).cursor, '2');
    await settle({
      type: 'catalog',
      items: [{ workKey: '/works/OL2W', title: 'Emma' }],
      nextCursor: null,
    });
    assert.equal(model.state.phase.page.items.length, 2);
    assert.equal(model.state.phase.page.nextCursor, null);
  });

  test('emptyKind distinguishes a typed miss from an empty browse page', () => {
    const base = { query: '', kind: 'books', session: 'token' };
    const empty = { type: 'books', items: [], nextCursor: null };
    const full = { type: 'books', items: [{ id: 'b' }], nextCursor: null };
    assert.equal(emptyKind({ ...base, phase: { status: 'idle' } }), 'none');
    assert.equal(emptyKind({ ...base, phase: { status: 'loading' } }), 'none');
    assert.equal(emptyKind({ ...base, phase: { status: 'failed', error: 'x' } }), 'none');
    assert.equal(emptyKind({ ...base, phase: { status: 'loaded', page: full, more: false } }), 'none');
    assert.equal(emptyKind({ ...base, phase: { status: 'loaded', page: empty, more: true } }), 'none');
    assert.equal(emptyKind({
      ...base, kind: 'accounts', phase: { status: 'loaded', page: empty, more: false },
    }), 'none');
    assert.equal(emptyKind({ ...base, phase: { status: 'loaded', page: empty, more: false } }), 'nothingYet');
    assert.equal(emptyKind({
      ...base, query: 'dune', phase: { status: 'loaded', page: empty, more: false },
    }), 'noMatches');
    assert.equal(isBrowse({ ...base, phase: { status: 'idle' } }), true);
    assert.equal(isBrowse({ ...base, query: '  ', phase: { status: 'idle' } }), true);
    assert.equal(isBrowse({ ...base, query: 'a', phase: { status: 'idle' } }), false);
    assert.equal(trendingNote([]), 'This week');
    assert.equal(trendingNote(['English']), 'This week · showing books with editions in English');
    assert.equal(
      trendingNote(['English', 'German']),
      'This week · showing books with editions in English and German',
    );
  });
});
