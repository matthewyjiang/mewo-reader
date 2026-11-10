import assert from 'node:assert/strict';
import { test } from 'node:test';
import { linkSpans } from '../src/lib/linkify.ts';

test('a url glued to the previous word is a link and trailing punctuation is not', () => {
  const text = 'supervision:https://en.wikipedia.org/wiki/Hopfield_network; https://en.wikipedia.org/wiki/Backpropagation.';
  const spans = linkSpans(text);
  assert.deepEqual(spans.map((span) => span.href), [
    'https://en.wikipedia.org/wiki/Hopfield_network',
    'https://en.wikipedia.org/wiki/Backpropagation',
  ]);
  assert.equal(text.slice(spans[0].start, spans[0].end), spans[0].href);
  assert.equal(text[spans[0].end], ';');
  assert.equal(text[spans[1].end], '.');
});

test('a trailing slash stays and the sentence period does not', () => {
  const text = 'microbes:https://www.ncbi.nlm.nih.gov/pmc/articles/PMC4790845/.';
  const spans = linkSpans(text);
  assert.deepEqual(spans, [{
    start: 'microbes:'.length,
    end: text.length - 1,
    href: 'https://www.ncbi.nlm.nih.gov/pmc/articles/PMC4790845/',
  }]);
});

test('prose around a url stays outside the link and the slash stays', () => {
  const text = 'go to https://www.ncbi.nlm.nih.gov/pmc/articles/ and at the search bar';
  const spans = linkSpans(text);
  assert.equal(spans.length, 1);
  assert.equal(spans[0].href, 'https://www.ncbi.nlm.nih.gov/pmc/articles/');
  assert.equal(text.slice(0, spans[0].start), 'go to ');
  assert.equal(text.slice(spans[0].end), ' and at the search bar');
});

test('an unmatched parenthesis inside the url is kept and the wrapping one is not', () => {
  const text = '(https://example.com/wiki/Hopfield_network_(neural)).';
  const spans = linkSpans(text);
  assert.deepEqual(spans.map((span) => span.href), [
    'https://example.com/wiki/Hopfield_network_(neural)',
  ]);
  assert.equal(text[spans[0].start - 1], '(');
  assert.equal(text.slice(spans[0].end), ').');
});

test('a label that is not a url is left alone', () => {
  assert.deepEqual(linkSpans('See this paper.'), []);
});
