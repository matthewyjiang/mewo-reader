import assert from 'node:assert/strict';
import { test } from 'node:test';
import { TimelineLayout } from '../src/lib/timeline.ts';

test('keeps measured heights and the same result anchor when rows arrive before it', () => {
  const layout = new TimelineLayout(['a', 'b', 'c']);
  layout.measure('a', 80);
  layout.measure('b', 240);
  const anchor = { key: 'b', index: 1, offset: 35 };
  layout.reset(['new', 'a', 'b', 'c']);
  assert.equal(layout.resolve(anchor), 2);
  assert.equal(layout.offset(2), 192 + 80);
  assert.equal(layout.offset(3) - layout.offset(2), 240);
  assert.equal(layout.indexAt(layout.offset(2) + anchor.offset), 2);
});

test('metadata changes do not change the dataset, and removal falls back to a nearby row', () => {
  const layout = new TimelineLayout(['a', 'b', 'c']);
  assert.equal(layout.matches(['a', 'b', 'c']), true);
  assert.equal(layout.matches(['a', 'c', 'b']), false);
  layout.reset(['a', 'c']);
  assert.equal(layout.resolve({ key: 'b', index: 1, offset: 0 }), 1);
  assert.equal(layout.resolve({ key: 'missing-tail', index: 99, offset: 0 }), 1);
});

test('height lookup stays consistent through reorder and further measurements', () => {
  const layout = new TimelineLayout(['a', 'b', 'c', 'd']);
  for (const [key, height] of [['a', 40], ['b', 120], ['c', 60], ['d', 300]]) layout.measure(key, height);
  layout.reset(['d', 'b', 'a', 'c']);
  assert.deepEqual([0, 1, 2, 3, 4].map(i => layout.offset(i)), [0, 300, 420, 460, 520]);
  layout.measure('b', 80);
  assert.deepEqual([0, 1, 2, 3, 4].map(i => layout.offset(i)), [0, 300, 380, 420, 480]);
  assert.deepEqual([0, 299, 300, 379, 380, 479].map(y => layout.indexAt(y)), [0, 0, 1, 1, 2, 3]);
  assert.throws(() => layout.reset(['duplicate', 'duplicate']), /keys must be unique/);
});
