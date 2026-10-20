import assert from 'node:assert/strict';
import { test } from 'node:test';
import { coverUrl, rowAction, stateLabel } from '../src/lib/requests.ts';

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

test('rowAction is request, pending, open, or retry', () => {
  assert.equal(rowAction(null), 'request');
  assert.equal(rowAction(request()), 'pending');
  assert.equal(rowAction(request({ state: 'downloading', progress: 42 })), 'pending');
  assert.equal(rowAction(request({ state: 'imported', bookId: 'req-1' })), 'open');
  assert.equal(rowAction(request({ state: 'failed', error: 'Download failed: missing' })), 'retry');
  assert.equal(rowAction(request({ state: 'canceled' })), 'retry');
});

test('coverUrl points at the Open Library cover CDN', () => {
  assert.equal(coverUrl(null), null);
  assert.equal(coverUrl(-1), null);
  assert.equal(coverUrl(11481354), 'https://covers.openlibrary.org/b/id/11481354-M.jpg?default=false');
  assert.equal(coverUrl(0), 'https://covers.openlibrary.org/b/id/0-M.jpg?default=false');
});
