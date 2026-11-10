import { expect, test, type Page, type Route } from '@playwright/test';

const hopfield = 'https://en.wikipedia.org/wiki/Hopfield_network';
const backprop = 'https://en.wikipedia.org/wiki/Backpropagation';
const microbes = 'https://www.ncbi.nlm.nih.gov/pmc/articles/PMC4790845/';
const note = `supervision: ${hopfield}; ${backprop}.`;
const glued = `microbes: ${microbes}.`;

const book = {
  id: 'projections',
  title: 'Projections',
  author: 'Karl Deisseroth',
  handle: 'deisseroth',
  importedAt: 1,
  postCount: 2,
  progressIndex: 0,
  isSample: false,
  mine: true,
  shared: false,
  ownerUsername: 'ada',
  held: false,
};

const posts = [
  { id: '0', kind: 'Body', text: note, chapter: 'Notes' },
  { id: '1', kind: 'Body', text: glued, chapter: 'Notes' },
];

async function install(page: Page): Promise<void> {
  await page.addInitScript(() => {
    localStorage.setItem('mewo.session', JSON.stringify({ token: 'token', username: 'ada' }));
    localStorage.setItem('mewo.display', JSON.stringify({
      theme: 'x_lights_out', step: 2, mewo: false, keepBars: true,
    }));
  });
  await page.route('**/v1/**', (route) => answer(route));
}

function answer(route: Route): Promise<void> {
  const url = new URL(route.request().url());
  const path = url.pathname;
  const method = route.request().method();
  const json = (body: unknown, status = 200) => route.fulfill({
    status,
    contentType: 'application/json',
    body: JSON.stringify(body),
  });
  if (path === '/v1/me') return json({ username: 'ada', isAdmin: false, canRequest: true });
  if (path === '/v1/library') return json({ books: [book] });
  if (path === '/v1/feed') {
    return json({
      items: [{
        type: 'comment',
        actor: { username: 'ada' },
        book,
        post: posts[0],
        comment: {
          id: 'c1',
          username: 'ada',
          text: `see ${hopfield}.`,
          createdAt: 1_700_000_000_000,
          mine: false,
        },
        at: 1_700_000_000_000,
      }],
      nextCursor: null,
    });
  }
  if (path === '/v1/books/projections/posts') return json({ posts });
  if (path === '/v1/books/projections/likes') return json({ ids: [] });
  if (path === '/v1/books/projections/comments') return json({ counts: {}, mine: [] });
  if (path === '/v1/books/projections/cover') return json({ error: 'Not found.' }, 404);
  if (path === '/v1/books/projections') return json(book);
  if (method === 'PATCH') return json(book);
  return json({ error: `unmocked ${method} ${path}` }, 500);
}

test('a reader post links its urls in a new tab', async ({ page }) => {
  await page.setViewportSize({ width: 390, height: 844 });
  await install(page);
  await page.goto('/books/projections');
  const post = page.locator('article.post').first();
  const link = post.locator(`a[href="${hopfield}"]`);
  await expect(link).toBeVisible();
  await expect(link).toHaveAttribute('href', hopfield);
  await expect(link).toHaveAttribute('target', '_blank');
  await expect(link).toHaveAttribute('rel', 'noopener noreferrer');
  await expect(link).toHaveText(hopfield);
  await expect(post.locator(`a[href="${backprop}"]`)).toHaveText(backprop);
  const body = post.locator('.post-body');
  await expect(body).toHaveJSProperty('tagName', 'DIV');
  await expect(body).toContainText(';');
  await expect(body).toContainText('.');
});

test('a feed card link does not also open the reader', async ({ page }) => {
  await page.setViewportSize({ width: 390, height: 844 });
  await install(page);
  await page.goto('/');
  const post = page.locator('article.post');
  const link = post.locator(`a[href="${hopfield}"]`);
  await expect(link).toBeVisible();
  await expect(post.locator('.post-body')).toHaveJSProperty('tagName', 'DIV');
  const reply = page.locator('.thread-row a[target="_blank"]');
  await expect(reply).toHaveAttribute('href', hopfield);
  const popup = page.waitForEvent('popup');
  await link.click();
  await expect(await popup).toHaveURL(hopfield);
  await expect(page).toHaveURL('/');
  await expect(page.locator('.thread-row')).toBeVisible();
});
