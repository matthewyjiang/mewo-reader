import assert from 'node:assert/strict';
import { test } from 'node:test';
import { searchLibrary } from '../src/lib/search.ts';

const book = (id, title = id) => ({
  id, title, author: 'Jane Austen', handle: 'jane', importedAt: 1, postCount: 0,
  progressIndex: 0, isSample: false, mine: true, shared: false, ownerUsername: 'reader',
});
const post = (id, text, chapter = '') => ({ id, text, chapter, kind: 'Body' });
const search = (books, feeds, query) => searchLibrary(books, feeds, query, new AbortController().signal);

test('matches metadata, text, and chapters without changing shelf or post order', async () => {
  const books = [book('one', 'Pride and Prejudice'), book('two', 'Persuasion')];
  const feeds = {
    one: [post('a', 'A quiet room'), post('b', 'A walk', 'Quiet mornings')],
    two: [post('c', 'QUIET waves')],
    removed: [post('d', 'Quiet but no longer on this shelf')],
  };
  assert.deepEqual((await search(books, feeds, ' quiet ')).posts.map(hit => hit.post.id), ['a', 'b', 'c']);
  assert.deepEqual((await search(books, feeds, 'PREJUDICE')).books, [books[0]]);
  assert.deepEqual((await search(books, feeds, 'Austen')).books, books);
  assert.deepEqual((await search(books, feeds, 'jane')).books, books);
  assert.deepEqual(await search(books, feeds, '   '), { books: [], posts: [] });
  assert.deepEqual(await search(books, feeds, 'absent'), { books: [], posts: [] });
});

test('yields before scanning so a superseded query can be cancelled', async () => {
  const controller = new AbortController();
  let reads = 0;
  const feeds = { get one() { reads++; return [post('a', 'quiet')]; } };
  const pending = searchLibrary([book('one')], feeds, 'quiet', controller.signal);
  assert.equal(reads, 0);
  controller.abort();
  assert.equal(await pending, null);
  assert.equal(reads, 0);
});

test('yields during a long scan and discards cancelled partial results', async (t) => {
  const controller = new AbortController();
  let clock = 0;
  // Advance the slice clock per post so this does not depend on machine speed.
  t.mock.method(performance, 'now', () => clock++);
  const posts = Array.from({ length: 2000 }, (_, i) => post(`${i}`, 'quiet'));
  let reads = 0;
  Object.defineProperty(posts[20], 'text', { get() {
    reads++;
    setTimeout(() => controller.abort(), 0);
    return 'quiet';
  } });
  assert.equal(await searchLibrary([book('one')], { one: posts }, 'quiet', controller.signal), null);
  assert.equal(reads, 1);
});

test('keeps every match and only searches feeds on the current shelf', async () => {
  const books = [book('one'), book('unopened')];
  const posts = Array.from({ length: 2000 }, (_, i) => post(`${i}`, 'quiet'));
  const results = await search(books, { one: posts }, 'quiet');
  assert.equal(results.posts.length, posts.length);
  assert.equal(results.posts.at(-1).post, posts.at(-1));
});
