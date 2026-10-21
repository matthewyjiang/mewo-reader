import assert from 'node:assert/strict';
import { test } from 'node:test';
import {
  actionErrorText, alsoRequested, badge, catalogErrorText, catalogRowLabel, copyLabel, coverUrl,
  heldCopy,
  noMatchCopy, openLabel, requestPhase, rowAction, stateLabel, subjectLine, workStats,
} from '../src/lib/requests.ts';

function request(over = {}) {
  return {
    id: 'req-1',
    workKey: '/works/OL893415W',
    title: 'Dune',
    author: 'Frank Herbert',
    year: 1965,
    coverId: 11481354,
    state: 'requested',
    progress: 0,
    error: null,
    bookId: null,
    requester: 'ada',
    createdAt: 0,
    updatedAt: 0,
    ...over,
  };
}

test('stateLabel distinguishes searching, waiting, and the other states', () => {
  assert.equal(stateLabel(request()), 'Searching');
  assert.equal(stateLabel(request({ error: 'Prowlarr: HTTP 500' })), 'Searching');
  assert.equal(stateLabel(request({ error: 'No release yet.' })), 'Waiting for a release');
  assert.equal(stateLabel(request({ state: 'downloading', progress: 42 })), 'Downloading 42%');
  assert.equal(stateLabel(request({ state: 'imported', bookId: 'req-1' })), 'On your shelf');
  assert.equal(stateLabel(request({ state: 'failed', error: 'No EPUB in the download.' })), 'Failed: No EPUB in the download.');
  assert.equal(stateLabel(request({ state: 'failed', error: null })), 'Failed: ');
  assert.equal(stateLabel(request({ state: 'canceled' })), 'Canceled');
});

test('rowAction is request, pending, open, retry, or again', () => {
  assert.equal(rowAction(null), 'request');
  assert.equal(rowAction(request()), 'pending');
  assert.equal(rowAction(request({ state: 'downloading', progress: 42 })), 'pending');
  assert.equal(rowAction(request({ state: 'imported', bookId: 'req-1' })), 'open');
  assert.equal(rowAction(request({ state: 'imported', bookId: null })), 'again');
  assert.equal(rowAction(request({ state: 'failed', error: 'Download failed: missing' })), 'retry');
  assert.equal(rowAction(request({ state: 'canceled' })), 'again');
});

test('catalogErrorText maps status codes and never ends in Try again', () => {
  assert.equal(
    catalogErrorText(0, 'Could not reach the server. Check your connection and try again.'),
    "Couldn't reach the server. Check your connection.",
  );
  assert.equal(catalogErrorText(401, 'expired'), 'Sign in again.');
  assert.equal(catalogErrorText(403, 'off'), 'Requests are off.');
  assert.equal(catalogErrorText(404, 'missing'), 'That book is no longer in the catalog.');
  assert.equal(
    catalogErrorText(502, "Couldn't search the catalog right now."),
    "Couldn't search the catalog right now.",
  );
  assert.equal(
    catalogErrorText(502, 'Open Library is not answering. Try again.'),
    'Open Library is not answering.',
  );
  assert.equal(catalogErrorText(500, 'Server returned 500. Try again.'), "Couldn't load the catalog.");
  assert.equal(catalogErrorText(500, 'HTTP 500'), "Couldn't load the catalog.");
  assert.equal(catalogErrorText(502, 'Prowlarr: HTTP 500'), 'Prowlarr: HTTP 500');
});

test('actionErrorText strips Try again without the catalog fallback', () => {
  assert.equal(
    actionErrorText(0, 'Could not reach the server. Check your connection and try again.'),
    "Couldn't reach the server. Check your connection.",
  );
  assert.equal(actionErrorText(401, 'expired'), 'Sign in again.');
  assert.equal(
    actionErrorText(500, 'Server returned 500. Try again.'),
    'Something went wrong.',
  );
  assert.equal(actionErrorText(500, 'HTTP 500'), 'Something went wrong.');
  assert.equal(
    actionErrorText(502, 'No indexer is configured. Try again.'),
    'No indexer is configured.',
  );
  assert.equal(actionErrorText(502, 'Prowlarr: HTTP 500'), 'Prowlarr: HTTP 500');
  assert.notEqual(actionErrorText(500, 'HTTP 500'), "Couldn't load the catalog.");
});

const held = {
  bookId: 'b',
  ownerUsername: 'secret',
  mine: false,
  shared: false,
  held: true,
};

const shared = {
  bookId: 'b',
  ownerUsername: 'alice',
  mine: false,
  shared: true,
  held: false,
};

test('stateLabel reads phase when the detail endpoint sent one', () => {
  assert.equal(stateLabel(request({ phase: 'searching' })), 'Searching');
  assert.equal(stateLabel(request({ phase: 'waiting' })), 'Waiting for a release');
  assert.equal(stateLabel(request({ phase: 'no_match' })), 'No matching release');
  assert.equal(stateLabel(request({
    phase: 'held', state: 'downloading', progress: 100, error: 'SABnzbd: paused',
  })), 'Waiting on SABnzbd');
  assert.equal(stateLabel(request({
    phase: 'downloading', state: 'downloading', progress: 40,
  })), 'Downloading 40%');
  assert.equal(stateLabel(request({
    phase: 'retrying', error: 'No EPUB in the download.',
  })), 'Retrying');
  assert.equal(stateLabel(request({
    phase: 'importing', state: 'downloading', progress: 100,
  })), 'Importing');
  assert.equal(stateLabel(request({ phase: 'imported', state: 'imported', bookId: 'req-1' })), 'On your shelf');
  assert.equal(stateLabel(request({ phase: 'failed', state: 'failed', error: 'No EPUB' })), 'Failed: No EPUB');
  assert.equal(stateLabel(request({ phase: 'canceled', state: 'canceled' })), 'Canceled');
});

test('requestPhase matches the server rule when the list row has no phase', () => {
  assert.equal(requestPhase(request()), 'searching');
  assert.equal(requestPhase(request({ error: 'No release yet.' })), 'waiting');
  assert.equal(requestPhase(request({
    error: 'No matching release (closest: Dune Messiah, a different book)',
  })), 'no_match');
  assert.equal(requestPhase(request({ error: 'SABnzbd: down' })), 'held');
  assert.equal(requestPhase(request({ error: 'Prowlarr: down' })), 'held');
  assert.equal(requestPhase(request({ error: 'No EPUB in the download.' })), 'retrying');
  assert.equal(requestPhase(request({ error: 'Download failed: CRC error' })), 'retrying');
  assert.equal(requestPhase(request({ error: '' })), 'searching');
  assert.equal(requestPhase(request({
    state: 'downloading', progress: 40, error: 'Open Library: HTTP 500',
  })), 'held');
  assert.equal(requestPhase(request({ state: 'downloading', progress: 40 })), 'downloading');
  assert.equal(requestPhase(request({ state: 'downloading', progress: 100 })), 'importing');
  assert.equal(requestPhase(request({
    state: 'downloading', progress: 100, error: 'SABnzbd: paused',
  })), 'held');
  assert.equal(requestPhase(request({ state: 'failed', error: 'No release found.' })), 'failed');
  assert.equal(requestPhase(request({ state: 'canceled' })), 'canceled');
  assert.equal(requestPhase(request({ state: 'imported', bookId: 'req-1' })), 'imported');
  assert.equal(requestPhase(request({
    phase: 'importing', state: 'downloading', progress: 40,
  })), 'importing');
});

test('badge prefers an active or failed request, and a held copy hides the owner', () => {
  const dune = {
    workKey: '/works/OL893415W',
    title: 'Dune',
    author: 'Frank Herbert',
    year: 1965,
    coverId: null,
    request: null,
    onServer: shared,
  };
  assert.equal(badge({ onServer: null }, null), null);
  assert.equal(badge({ onServer: shared }, null), 'On server · @alice');
  assert.equal(badge({ onServer: held }, null), 'On your shelf');
  assert.equal(copyLabel(held, 'badge').includes('secret'), false);
  assert.equal(copyLabel(held, 'sheet'), 'On your shelf');
  assert.equal(copyLabel({ ...held, shared: true, ownerUsername: 'secret' }, 'sheet'), 'On your shelf');
  assert.equal(copyLabel(shared, 'sheet'), 'On server, shared by @alice');
  assert.equal(copyLabel({ ...shared, ownerUsername: '' }, 'badge'), 'On server');
  assert.equal(badge({ onServer: shared }, request()), 'Searching');
  assert.equal(badge({ onServer: shared }, request({
    phase: 'downloading', state: 'downloading', progress: 40,
  })), 'Downloading 40%');
  assert.equal(badge({ onServer: shared }, request({ state: 'failed', error: 'nope' })), 'Failed: nope');
  assert.equal(badge({ onServer: shared }, request({ state: 'canceled' })), 'On server · @alice');
  assert.equal(badge({ onServer: null }, request({ state: 'imported', bookId: 'req-1' })), 'On your shelf');
  assert.equal(catalogRowLabel(dune, null), 'Dune, Frank Herbert, 1965, On server · @alice');
  assert.equal(catalogRowLabel({ ...dune, onServer: held }, null), 'Dune, Frank Herbert, 1965, On your shelf');
  assert.equal(catalogRowLabel({ ...dune, onServer: held }, null).includes('secret'), false);
});

test('rowAction opens a server copy and keeps a failed request on retry', () => {
  assert.equal(rowAction(null, shared), 'open');
  assert.equal(rowAction(request(), shared), 'pending');
  assert.equal(rowAction(request({ state: 'failed', error: 'x' }), shared), 'retry');
  assert.equal(rowAction(request({ state: 'canceled' }), shared), 'open');
  assert.equal(rowAction(request({ state: 'imported', bookId: null }), shared), 'open');
  assert.equal(openLabel({ onServer: shared }, null), 'On server');
  assert.equal(openLabel({ onServer: held }, null), 'On your shelf');
  assert.equal(openLabel({ onServer: null }, request({ state: 'imported', bookId: 'req-1' })), 'On your shelf');
});

test('sheet copy names the closest book and the held service', () => {
  const error = 'No matching release (closest: Dune Messiah, a different book)';
  assert.match(noMatchCopy(error, null), /Dune Messiah/);
  assert.match(noMatchCopy(error, 1_700_000_000_000), /Searching again/);
  assert.equal(heldCopy('SABnzbd: paused'), 'Waiting on SABnzbd: paused');
  assert.equal(heldCopy('Prowlarr: down'), 'Waiting on Prowlarr: down');
});

test('also requested names three people and counts the rest', () => {
  assert.equal(alsoRequested(['lin']), 'Also requested by @lin');
  assert.equal(alsoRequested(['lin', 'ada', 'noor']), 'Also requested by @lin, @ada, @noor');
  assert.equal(alsoRequested(['lin', 'ada', 'noor', 'grace']), 'Also requested by @lin, @ada, @noor +1');
});

test('sheet stats omit edition count, rating count, and languages', () => {
  assert.equal(workStats({
    year: 1965,
    pagesMedian: 608,
    rating: { average: 4.3 },
  }), 'First published 1965 · ~608 pages · ★ 4.3');
  assert.equal(workStats({ year: null, pagesMedian: null, rating: null }), '');
  assert.equal(
    subjectLine(['Science fiction', 'Arrakis', 'Deserts', 'Sand', 'Politics', 'Ecology']),
    'Science fiction · Arrakis · Deserts · Sand · Politics',
  );
});

test('coverUrl points at the Open Library cover CDN', () => {
  assert.equal(coverUrl(null), null);
  assert.equal(coverUrl(-1), null);
  assert.equal(coverUrl(11481354), 'https://covers.openlibrary.org/b/id/11481354-M.jpg?default=false');
  assert.equal(coverUrl(0), 'https://covers.openlibrary.org/b/id/0-M.jpg?default=false');
});
