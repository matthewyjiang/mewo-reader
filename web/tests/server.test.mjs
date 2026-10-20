import assert from 'node:assert/strict';
import { test } from 'node:test';
import { EPUB_MB_ERROR, MB, bytesToMb, epubMbError, formatBytes, parseMb } from '../src/lib/server.ts';

test('megabytes convert both ways and reject junk', () => {
  assert.equal(bytesToMb(64 * MB), 64);
  assert.equal(parseMb('64'), 64 * MB);
  assert.equal(parseMb('  64  '), 64 * MB);
  assert.equal(parseMb('64.0'), 64 * MB);
  assert.equal(parseMb('1.5'), Math.round(1.5 * MB));
  assert.equal(parseMb(''), null);
  assert.equal(parseMb('abc'), null);
  assert.equal(parseMb('-1'), null);
  assert.equal(parseMb('0'), null);
  assert.equal(parseMb('+64'), null);
  assert.equal(parseMb('1e2'), null);
  assert.equal(parseMb('64.'), null);
});

test('epub megabytes are 1 to 1024', () => {
  assert.equal(epubMbError(MB), null);
  assert.equal(epubMbError(64 * MB), null);
  assert.equal(epubMbError(1024 * MB), null);
  assert.equal(epubMbError(parseMb('0.5')), EPUB_MB_ERROR);
  assert.equal(epubMbError(parseMb('1025')), EPUB_MB_ERROR);
  assert.equal(EPUB_MB_ERROR, 'maxEpubBytes must be 1 to 1024 MB.');
});

test('formatBytes uses one decimal only when the unit is not exact', () => {
  assert.equal(formatBytes(0), '0 B');
  assert.equal(formatBytes(512), '512 B');
  assert.equal(formatBytes(1024), '1 KB');
  assert.equal(formatBytes(1536), '1.5 KB');
  assert.equal(formatBytes(40_500_000), '38.6 MB');
  assert.equal(formatBytes(64 * MB), '64 MB');
  assert.equal(formatBytes(1024 * MB), '1 GB');
  assert.equal(formatBytes(-1), '0 B');
});
