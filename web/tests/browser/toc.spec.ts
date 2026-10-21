import { expect, test, type Locator, type Page, type Route } from '@playwright/test';

const posts = Array.from({ length: 40 }, (_, index) => {
  const chapter = `${index + 1}. Chapter title number ${index + 1}`;
  return [
    { id: String(index * 2), kind: 'Heading', text: chapter, chapter },
    {
      id: String(index * 2 + 1),
      kind: 'Body',
      text: `Paragraph ${index + 1} of The Problem of Pain.`,
      chapter,
    },
  ];
}).flat();

const book = {
  id: 'pain',
  title: 'The Problem of Pain',
  author: 'C. S. Lewis',
  handle: 'cslewis',
  importedAt: 1,
  postCount: posts.length,
  progressIndex: 0,
  isSample: false,
  mine: true,
  shared: false,
  ownerUsername: 'ada',
  held: false,
};

async function install(page: Page): Promise<void> {
  await page.addInitScript(() => {
    localStorage.setItem('mewo.session', JSON.stringify({ token: 'token', username: 'ada' }));
    localStorage.setItem('mewo.display', JSON.stringify({
      theme: 'x_light', step: 2, mewo: false, keepBars: true,
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
  if (path === '/v1/feed') return json({ items: [], nextCursor: null });
  if (path === '/v1/books/pain/posts') return json({ posts });
  if (path === '/v1/books/pain/likes') return json({ ids: [] });
  if (path === '/v1/books/pain/comments') return json({ counts: {}, mine: [] });
  if (path === '/v1/books/pain/cover') return json({ error: 'Not found.' }, 404);
  if (path === '/v1/books/pain') return json(book);
  if (method === 'PATCH') return json(book);
  return json({ error: `unmocked ${method} ${path}` }, 500);
}

async function openToc(page: Page): Promise<Locator> {
  await install(page);
  await page.goto('/books/pain');
  await page.getByRole('button', { name: 'Chapters, The Problem of Pain' }).click();
  const dialog = page.getByRole('dialog', { name: 'Chapters, The Problem of Pain' });
  await expect(dialog).toBeVisible();
  await expect(dialog.getByRole('button', { name: '1. Chapter title number 1', exact: true })).toBeVisible();
  return dialog;
}

async function scrollers(dialog: Locator): Promise<string[]> {
  return dialog.evaluate((node) => {
    const elements = [node, ...node.querySelectorAll<HTMLElement>('*')];
    return elements.filter((el) => {
      const y = getComputedStyle(el).overflowY;
      if (y !== 'auto' && y !== 'scroll') return false;
      return el.scrollHeight > el.clientHeight + 1;
    }).map((el) => el.className);
  });
}

test('the desktop chapter sheet has one scroller and sits in the center', async ({ page }) => {
  await page.setViewportSize({ width: 1280, height: 662 });
  const dialog = await openToc(page);
  const box = await dialog.boundingBox();
  expect(box).not.toBeNull();
  const top = box!.y;
  const height = box!.height;
  const expected = (662 - height) / 2;
  expect(Math.abs(top - expected)).toBeLessThan(2);
  expect(top).toBeGreaterThan(24);
  expect(top + height).toBeLessThan(662 - 24);
  const scrolling = await scrollers(dialog);
  expect(scrolling).toHaveLength(1);
  expect(scrolling[0]).toContain('chapter-list');
  const moved = await dialog.locator('.chapter-list').evaluate((node) => {
    const before = node.scrollTop;
    node.scrollTop = before + 200;
    return node.scrollTop - before;
  });
  expect(moved).toBeGreaterThan(0);
});

test('the phone chapter sheet stays on the bottom and still scrolls once', async ({ page }) => {
  await page.setViewportSize({ width: 390, height: 844 });
  const dialog = await openToc(page);
  const box = await dialog.boundingBox();
  expect(box).not.toBeNull();
  expect(box!.y + box!.height).toBeGreaterThan(844 - 2);
  expect(box!.y).toBeGreaterThan(80);
  const scrolling = await scrollers(dialog);
  expect(scrolling).toHaveLength(1);
  expect(scrolling[0]).toContain('chapter-list');
});
