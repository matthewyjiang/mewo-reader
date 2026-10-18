import assert from 'node:assert/strict';
import { test } from 'node:test';
import { commitRepost, hasReposted, repostCount } from '../src/lib/repost.ts';

const index = (reposts, reposted) => ({ counts: {}, mine: [], reposts, reposted });

test('a failed repost rolls back unless a newer toggle landed', async () => {
  let current = index({}, []);
  await assert.rejects(commitRepost(
    current,
    '1',
    async () => { throw new Error('offline'); },
    () => current,
    (next) => { current = next; },
  ));
  assert.equal(hasReposted(current, '1'), false);
  assert.equal(repostCount(current, '1'), 0);

  let raced = index({}, []);
  const pending = commitRepost(
    raced,
    '1',
    () => new Promise((_, reject) => setTimeout(() => reject(new Error('offline')), 10)),
    () => raced,
    (next) => { raced = next; },
  );
  raced = index({ '1': 9 }, []);
  await assert.rejects(pending);
  assert.equal(repostCount(raced, '1'), 9);
  assert.equal(hasReposted(raced, '1'), false);
});

test('the server count replaces the optimistic one', async () => {
  let current = index({ '1': 2 }, []);
  await commitRepost(
    current,
    '1',
    async () => ({ reposts: 4, reposted: true }),
    () => current,
    (next) => { current = next; },
  );
  assert.equal(repostCount(current, '1'), 4);
  assert.equal(hasReposted(current, '1'), true);
  assert.deepEqual(current.mine, []);
});
