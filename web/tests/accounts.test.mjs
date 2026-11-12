import assert from 'node:assert/strict';
import { test } from 'node:test';
import { removalLines } from '../src/lib/accounts.ts';

const none = {
  booksDeleted: 0,
  sharedDeleted: 0,
  booksPassed: 0,
  holdsReleased: 0,
  requestsCanceled: 0,
};

test('removalLines leaves out zero counts', () => {
  assert.deepEqual(removalLines(none), ['Removes the account, its replies, and its reposts.']);
});

test('removalLines uses plurals and names what others keep', () => {
  assert.deepEqual(removalLines({
    booksDeleted: 3,
    sharedDeleted: 2,
    booksPassed: 2,
    holdsReleased: 2,
    requestsCanceled: 2,
  }), [
    'Deletes 3 books, 2 that others can read.',
    '2 books stay with the people who also have them.',
    'Gives back 2 books someone else added.',
    'Cancels 2 requests.',
    'Removes the account, its replies, and its reposts.',
  ]);
  assert.deepEqual(removalLines({
    booksDeleted: 1,
    sharedDeleted: 0,
    booksPassed: 1,
    holdsReleased: 1,
    requestsCanceled: 1,
  }), [
    'Deletes 1 book.',
    '1 book stays with the person who also has it.',
    'Gives back 1 book someone else added.',
    'Cancels 1 request.',
    'Removes the account, its replies, and its reposts.',
  ]);
});
