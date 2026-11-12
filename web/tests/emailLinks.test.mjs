import assert from 'node:assert/strict';
import { test } from 'node:test';
import { linkToken, withoutHash } from '../src/lib/emailLinks.ts';

test('linkToken reads the token after #token=', () => {
  assert.equal(linkToken('https://mewo.example/reset#token=abc123'), 'abc123');
  assert.equal(linkToken(new URL('https://mewo.example/verify-email#token=f00')), 'f00');
  assert.equal(linkToken('https://mewo.example/reset#x=1&token=abc'), 'abc');
});

test('linkToken ignores a query token and blank or missing hashes', () => {
  assert.equal(linkToken('https://mewo.example/reset?token=abc'), null);
  assert.equal(linkToken('https://mewo.example/reset'), null);
  assert.equal(linkToken('https://mewo.example/reset#token='), null);
  assert.equal(linkToken('https://mewo.example/reset#token=%20'), null);
  assert.equal(linkToken('not a url'), null);
});

test('withoutHash keeps the path and query', () => {
  assert.equal(withoutHash('https://mewo.example/reset?a=1#token=abc'), '/reset?a=1');
  assert.equal(withoutHash('https://mewo.example/verify-email#token=abc'), '/verify-email');
});
