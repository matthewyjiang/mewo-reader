import assert from 'node:assert/strict';
import { test } from 'node:test';
import { CATALOG_DEBOUNCE_MS, SearchModel, SEARCH_DEBOUNCE_MS, showNoMatches } from '../src/lib/search.ts';

function deferred() {
  let resolve;
  let reject;
  const promise = new Promise((res, rej) => { resolve = res; reject = rej; });
  return { promise, resolve, reject };
}

function harness(session = 'token', kind = 'accounts') {
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
  );
  return {
    model,
    calls,
    timers,
    replace() { pending = deferred(); },
    fire() {
      const id = timers.shift();
      assert.equal(id.ms, kind === 'catalog' ? CATALOG_DEBOUNCE_MS : SEARCH_DEBOUNCE_MS);
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

test('no matches waits for a successful empty page', async () => {
  const { model, fire, settle } = harness();
  model.setQuery('ada');
  assert.equal(showNoMatches(model.state), false);
  assert.equal(model.state.phase.status, 'loading');
  fire();
  assert.equal(showNoMatches(model.state), false);
  await settle(emptyAccounts);
  assert.equal(showNoMatches(model.state), true);
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
  assert.equal(showNoMatches(model.state), false);
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
  assert.equal(showNoMatches(model.state), false);
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
