import assert from 'node:assert/strict';
import { test } from 'node:test';
import { relativeTime } from '../src/lib/models.ts';

const now = Date.UTC(2026, 9, 18, 12, 0, 0);

test('a recent comment stays short and an older one is a date', () => {
  assert.equal(relativeTime(now, now), 'now');
  assert.equal(relativeTime(now + 60_000, now), 'now');
  assert.equal(relativeTime(now - 5 * 60_000, now), '5m');
  assert.equal(relativeTime(now - 3 * 3_600_000, now), '3h');
  assert.equal(relativeTime(now - 23 * 3_600_000, now), '23h');
  assert.equal(relativeTime(Date.UTC(2026, 9, 3, 15, 0, 0), now), 'Oct 3');
  assert.equal(relativeTime(now - 24 * 3_600_000, now), 'Oct 17');
  assert.equal(relativeTime(Date.UTC(2024, 9, 3, 15, 0, 0), now), 'Oct 3, 2024');
  assert.equal(relativeTime(5, now), 'Jan 1, 1970');
});
