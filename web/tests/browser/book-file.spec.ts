import { expect, test, type Page } from '@playwright/test';

const book = {
  id: 'dune',
  title: 'Dune',
  author: 'Frank Herbert',
  mine: true,
  held: true,
  shared: true,
  isSample: false,
  progressIndex: 0,
  progressTotal: 12,
  hasRelease: true,
  reportable: true,
  feedRev: 0,
};

const sample = {
  ...book,
  id: 'sample',
  title: 'Sample',
  isSample: true,
  hasRelease: false,
  reportable: false,
};

const blocked = {
  id: 'block-1',
  guid: 'g1',
  indexer: 'mock',
  title: 'Dune retail',
  size: 80000,
  workKey: '/works/OL893414W',
  bookTitle: 'Dune',
  bookAuthor: 'Frank Herbert',
  createdAt: 1,
};

test.beforeEach(async ({ page }) => {
  let reported = false;
  let blockedRows = [blocked];
  await page.addInitScript(() => {
    localStorage.setItem('mewo.session', JSON.stringify({
      token: 'tok', username: 'matt', expiresAt: '2999-01-01T00:00:00Z',
    }));
    if (!localStorage.getItem('mewo.display')) {
      localStorage.setItem('mewo.display', JSON.stringify({ theme: 'x_light', step: 2 }));
    }
  });
  await page.route('**/v1/**', async (route) => {
    const url = new URL(route.request().url());
    const path = url.pathname;
    const method = route.request().method();
    const json = (body: unknown, status = 200) => route.fulfill({
      status,
      contentType: 'application/json',
      body: JSON.stringify(body),
    });
    if (path === '/v1/me') {
      return json({ username: 'matt', isAdmin: true, canRequest: true });
    }
    if (path === '/v1/library') return json({ books: reported ? [] : [book, sample] });
    if (path === '/v1/feed') return json({ items: [], nextCursor: null });
    if (path === '/v1/books/dune/wrong-file' && method === 'POST') {
      reported = true;
      return json({
        id: 'req-1', workKey: '/works/OL893414W', title: 'Dune', author: 'Frank Herbert',
        state: 'requested', progress: 0,
      }, 202);
    }
    if (path === '/v1/books/dune/epub' && method === 'PUT') {
      if (url.searchParams.get('force') === '1') return json({ ...book, feedRev: 1 });
      return json({ error: 'This file matches 0% of the current book.' }, 409);
    }
    if (path === '/v1/admin/blocklist' && method === 'GET') return json({ items: blockedRows });
    if (path === '/v1/admin/blocklist/block-1' && method === 'DELETE') {
      blockedRows = [];
      return route.fulfill({ status: 204, body: '' });
    }
    return json({});
  });
});

async function theme(page: Page, name: string): Promise<void> {
  await page.evaluate((next) => {
    localStorage.setItem('mewo.display', JSON.stringify({ theme: next, step: 2 }));
  }, name);
}

async function openSheet(page: Page): Promise<void> {
  await page.goto('/?tab=books');
  await expect(page.getByText('Sample', { exact: true })).toBeVisible();
  await expect(page.getByRole('button', { name: 'Book file for Sample' })).toHaveCount(0);
  await page.getByRole('button', { name: 'Book file for Dune' }).click();
  await expect(page.getByRole('heading', { name: 'Book file' })).toBeVisible();
  await expect(page.getByText('Upload replacement')).toBeVisible();
  await expect(page.getByRole('button', { name: 'Wrong file' })).toBeVisible();
}

test('book file and blocked releases', async ({ page }) => {
  await page.setViewportSize({ width: 1280, height: 800 });
  await openSheet(page);
  await page.screenshot({ path: '/opt/cursor/artifacts/book-file-light.png' });
  await page.getByRole('button', { name: 'Wrong file' }).click();
  await expect(page.getByRole('heading', { name: 'Delete this copy?' })).toBeVisible();
  await expect(page.getByText(
    'Replies, likes and reading progress on it are deleted for everyone who has it. Mewo then looks for another copy.',
  )).toBeVisible();
  await page.screenshot({ path: '/opt/cursor/artifacts/wrong-file-confirm-light.png' });
  await page.getByRole('button', { name: 'Keep' }).click();

  await theme(page, 'x_lights_out');
  await openSheet(page);
  await page.screenshot({ path: '/opt/cursor/artifacts/book-file-dark.png' });

  await page.setViewportSize({ width: 390, height: 844 });
  await openSheet(page);
  await page.screenshot({ path: '/opt/cursor/artifacts/book-file-narrow.png' });

  await page.getByRole('button', { name: 'Wrong file' }).click();
  await expect(page.getByRole('heading', { name: 'Delete this copy?' })).toBeVisible();
  await page.getByRole('button', { name: 'Keep' }).click();
  await expect(page.getByRole('heading', { name: 'Book file' })).toBeVisible();

  await page.getByRole('button', { name: 'Upload replacement' }).setInputFiles({
    name: 'other.epub',
    mimeType: 'application/epub+zip',
    buffer: Buffer.from('not the same book'),
  });
  await expect(page.getByRole('button', { name: 'Replace anyway' })).toBeVisible();
  await expect(page.getByRole('alert')).toContainText('matches');
  await page.getByRole('button', { name: 'Replace anyway' }).click();
  await expect(page.getByRole('heading', { name: 'Book file' })).toHaveCount(0);

  await openSheet(page);
  await page.getByRole('button', { name: 'Wrong file' }).click();
  await page.getByRole('button', { name: 'Delete and search' }).click();
  await expect(page.getByRole('heading', { name: 'No books' })).toBeVisible();

  await page.setViewportSize({ width: 1280, height: 800 });
  await theme(page, 'x_light');
  await page.goto('/settings/server/blocklist');
  await expect(page.getByRole('button', { name: 'Unblock' })).toBeVisible();
  await expect(page.getByText('Dune retail', { exact: true })).toBeVisible();
  await page.screenshot({ path: '/opt/cursor/artifacts/blocklist-light.png' });

  await theme(page, 'x_lights_out');
  await page.goto('/settings/server/blocklist');
  await expect(page.getByRole('button', { name: 'Unblock' })).toBeVisible();
  await page.screenshot({ path: '/opt/cursor/artifacts/blocklist-dark.png' });

  await page.setViewportSize({ width: 390, height: 844 });
  await page.goto('/settings/server/blocklist');
  await expect(page.getByRole('button', { name: 'Unblock' })).toBeVisible();
  await page.screenshot({ path: '/opt/cursor/artifacts/blocklist-narrow.png' });

  await page.getByRole('button', { name: 'Unblock' }).click();
  await expect(page.getByText('No blocked releases.')).toBeVisible();
});
