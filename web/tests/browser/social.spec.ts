import { expect, test, type Page, type Route } from '@playwright/test';
import { mkdir } from 'node:fs/promises';

const book = {
  id: 'b1',
  title: 'Emma',
  author: 'Austen',
  handle: 'austen',
  importedAt: 1,
  postCount: 3,
  progressIndex: 0,
  isSample: false,
  mine: true,
  shared: true,
  ownerUsername: 'ada',
};

const privateBook = {
  ...book,
  id: 'b2',
  title: 'Persuasion',
  shared: false,
};

const heldBook = {
  ...book,
  id: 'b3',
  title: 'Mansfield',
  mine: false,
  shared: false,
  held: true,
  ownerUsername: 'secret',
};

const sharedBook = {
  ...book,
  id: 'b4',
  title: 'Sanditon',
  mine: false,
  shared: true,
  held: false,
  ownerUsername: 'mary',
};

const namelessBook = {
  ...book,
  id: 'b5',
  title: 'Lady Susan',
  mine: false,
  shared: true,
  ownerUsername: '',
};

const post = {
  id: '0',
  kind: 'Body',
  text: 'Emma Woodhouse, handsome, clever, and rich.',
  chapter: 'One',
};

/** Milliseconds, same as `comments.created_at`. 2024-10-03T15:00:00Z. */
const commentAt = Date.UTC(2024, 9, 3, 15, 0, 0);

const comment = {
  id: 'c1',
  username: 'ada',
  text: 'This opening still lands.',
  createdAt: commentAt,
  mine: true,
};

let reposted = false;
const removed = new Set<string>();

async function install(page: Page, theme: 'x_light' | 'x_lights_out' = 'x_light') {
  reposted = false;
  removed.clear();
  await page.addInitScript((nextTheme) => {
    localStorage.setItem('mewo.session', JSON.stringify({ token: 'token', username: 'ada' }));
    localStorage.setItem('mewo.display', JSON.stringify({
      theme: nextTheme, step: 2, mewo: false, keepBars: true,
    }));
  }, theme);
  await page.route('**/v1/**', (route) => answer(route));
}

async function answer(route: Route) {
  const url = new URL(route.request().url());
  const path = url.pathname;
  const method = route.request().method();
  const json = (body: unknown, status = 200) => route.fulfill({
    status,
    contentType: 'application/json',
    body: JSON.stringify(body),
  });
  if (path === '/v1/library') {
    const books = [book, privateBook, heldBook, sharedBook, namelessBook]
      .filter((row) => !removed.has(row.id));
    return json({ books });
  }
  if (method === 'DELETE' && path.startsWith('/v1/books/')) {
    removed.add(path.slice('/v1/books/'.length));
    return json({});
  }
  if (path === '/v1/feed') {
    return json({
      items: [
        {
          type: 'repost',
          actor: { username: 'byron' },
          book,
          post,
          at: commentAt + 1000,
        },
        {
          type: 'comment',
          actor: { username: 'ada' },
          book,
          post,
          comment,
          at: commentAt,
        },
      ],
      nextCursor: null,
    });
  }
  if (path === '/v1/search') {
    const type = url.searchParams.get('type');
    if (type === 'books') return json({ type: 'books', items: [book], nextCursor: null });
    if (type === 'posts') return json({ type: 'posts', items: [{ book, post }], nextCursor: null });
    const q = (url.searchParams.get('q') ?? '').toLowerCase();
    const accounts = [
      { username: 'ada', mine: true },
      { username: 'byron', mine: false },
    ].filter((account) => account.username.includes(q));
    return json({ type: 'accounts', items: accounts, nextCursor: null });
  }
  if (path === '/v1/users/ada') {
    return json({
      username: 'ada', joinedAt: 1609459200000, postCount: 2, repostCount: 1, mine: true,
    });
  }
  if (path === '/v1/users/ada/posts') {
    return json({ items: [{ book, post, comment }], nextCursor: null });
  }
  if (path === '/v1/users/ada/reposts') {
    return json({ items: [{ book, post, repostedAt: commentAt }], nextCursor: null });
  }
  if (path === '/v1/users/ada/books') return json({ items: [book] });
  if (path === '/v1/users/nobody') return json({ error: 'Not found.' }, 404);
  if (path === '/v1/books/b1/comments') {
    return json({
      counts: { '0': 1 },
      mine: ['0'],
      // Byron already reposted this line. The viewer's own repost adds one.
      reposts: { '0': reposted ? 2 : 1 },
      reposted: reposted ? ['0'] : [],
    });
  }
  if (path === '/v1/books/b1/likes') return json({ ids: [] });
  if (path === '/v1/books/b1/posts') return json({ posts: [post] });
  if (path === '/v1/books/b1/posts/0/repost') {
    reposted = method === 'PUT';
    return json({ reposts: reposted ? 2 : 1, reposted });
  }
  if (path.endsWith('/cover')) return route.fulfill({ status: 404, body: '' });
  return json({});
}

test('search tabs keep each kind of hit', async ({ page }) => {
  await page.setViewportSize({ width: 390, height: 844 });
  await install(page);
  await page.goto('/search');
  await page.getByRole('searchbox').fill('ada');
  await expect(page.getByRole('button', { name: '@ada' })).toBeVisible();
  await expect(page.getByRole('button', { name: '@byron' })).toHaveCount(0);
  await page.getByRole('searchbox').fill('byr');
  await expect(page.getByRole('button', { name: '@byron' })).toBeVisible();
  await expect(page.getByRole('button', { name: '@ada' })).toHaveCount(0);
  await page.getByRole('searchbox').fill('ada');
  await expect(page.getByText('No matches')).toHaveCount(0);
  await page.getByRole('tab', { name: 'Books' }).click();
  await expect(page.getByText('Emma', { exact: true })).toBeVisible();
  await page.getByRole('tab', { name: 'Posts' }).click();
  await expect(page.getByText('Emma Woodhouse, handsome, clever, and rich.')).toBeVisible();
});

test('profile tabs and a missing account', async ({ page }) => {
  await page.setViewportSize({ width: 390, height: 844 });
  await install(page);
  await page.goto('/profiles/ada');
  await expect(page.getByText('Joined Jan 2021')).toBeVisible();
  await expect(page.getByText('This opening still lands.')).toBeVisible();
  await page.getByRole('tab', { name: 'Reposts' }).click();
  await expect(page).toHaveURL(/tab=reposts/);
  await expect(page.getByText('Emma Woodhouse, handsome, clever, and rich.')).toBeVisible();
  await page.getByRole('tab', { name: 'Books' }).click();
  await expect(page).toHaveURL(/tab=books/);
  await expect(page.getByText('Emma', { exact: true })).toBeVisible();
  await expect(page.getByText('Persuasion')).toHaveCount(0);
  await page.goto('/profiles/nobody');
  await expect(page.getByText("This account doesn't exist")).toBeVisible();
});

test('a repost stays on after reload', async ({ page }) => {
  await page.setViewportSize({ width: 390, height: 844 });
  await install(page);
  await page.goto('/search');
  await page.getByRole('tab', { name: 'Posts' }).click();
  await page.getByRole('searchbox').fill('line');
  const repost = page.getByRole('button', { name: 'Reposts, 1' });
  await expect(repost).toBeEnabled();
  await expect(repost).toHaveAttribute('aria-pressed', 'false');
  await repost.click();
  const mine = page.getByRole('button', { name: 'Reposts, 2' });
  await expect(mine).toBeEnabled();
  await expect(mine).toHaveAttribute('aria-pressed', 'true');
  await page.reload();
  await page.getByRole('searchbox').fill('line');
  await expect(page.getByRole('button', { name: 'Reposts, 2' })).toBeEnabled();
  await expect(page.getByRole('button', { name: 'Reposts, 2' })).toHaveAttribute('aria-pressed', 'true');
});

test('a repost stays faded only while the request is open', async ({ page }) => {
  await page.setViewportSize({ width: 390, height: 844 });
  await install(page);
  let release: () => void = () => {};
  const gate = new Promise<void>((resolve) => { release = resolve; });
  await page.route('**/v1/**/repost', async (route) => {
    if (route.request().method() === 'PUT') await gate;
    await route.fallback();
  });
  await page.goto('/search');
  await page.getByRole('tab', { name: 'Posts' }).click();
  await page.getByRole('searchbox').fill('line');
  await page.getByRole('button', { name: 'Reposts, 1' }).click();
  const pending = page.getByRole('button', { name: 'Reposts, 2' });
  await expect(pending).toBeDisabled();
  release();
  await expect(pending).toBeEnabled();
  await expect(pending).toHaveAttribute('aria-pressed', 'true');
});

test('home opens on Feed and Books keeps the shelf', async ({ page }) => {
  await page.setViewportSize({ width: 390, height: 844 });
  await install(page);
  await page.goto('/');
  await expect(page.getByRole('tab', { name: 'Feed' })).toHaveAttribute('aria-selected', 'true');
  await expect(page.getByText('byron reposted')).toBeVisible();
  await expect(page.getByText('Oct 3, 2024')).toBeVisible();
  await expect(page.getByText('This opening still lands.')).toBeVisible();
  const counts = page.getByRole('button', { name: 'Reposts, 1' });
  await expect(counts).toHaveCount(2);
  for (const button of await counts.all()) {
    await expect(button).toBeEnabled();
    await expect(button).toHaveAttribute('aria-pressed', 'false');
  }
  await page.getByRole('tab', { name: 'Books' }).click();
  await expect(page).toHaveURL(/tab=books/);
  await expect(page.getByText('Emma', { exact: true })).toBeVisible();
  await expect(page.getByText('Persuasion')).toBeVisible();
  await expect(page.getByText('Mansfield', { exact: true })).toBeVisible();
  await expect(page.getByText('Sanditon', { exact: true })).toBeVisible();
  await expect(page.getByText('Lady Susan', { exact: true })).toBeVisible();
  await expect(page.getByText('Shared by @mary')).toBeVisible();
  await expect(page.getByText(/Added by/)).toHaveCount(0);
  await expect(page.getByText('secret')).toHaveCount(0);
  await expect(page.getByText('Shared by @', { exact: true })).toHaveCount(0);
  await expect(page.getByRole('button', { name: 'Remove Emma' })).toBeVisible();
  const held = page.locator('.book-row', { hasText: 'Mansfield' });
  await expect(held.getByRole('button', { name: 'Remove from library' })).toBeVisible();
  await expect(held.getByRole('button', { name: 'Share' })).toHaveCount(0);
  await expect(held.getByRole('button', { name: 'Make private' })).toHaveCount(0);
  await held.getByRole('button', { name: 'Remove from library' }).click();
  const sheet = page.getByRole('dialog', { name: 'Remove book' });
  await expect(sheet).toContainText('Only removes it for you.');
  const deleted = page.waitForRequest((request) =>
    request.method() === 'DELETE' && request.url().includes('/v1/books/b3'));
  await sheet.getByRole('button', { name: 'Remove', exact: true }).click();
  await deleted;
  await expect(page.getByText('Mansfield', { exact: true })).toHaveCount(0);
  await expect(page.getByText('Emma', { exact: true })).toBeVisible();
});

test('the drawer Profile row opens your profile', async ({ page }) => {
  await page.setViewportSize({ width: 390, height: 844 });
  await install(page);
  await page.goto('/');
  await page.getByRole('button', { name: 'Open account drawer' }).click();
  await expect(page.getByRole('button', { name: 'Settings' })).toBeVisible();
  await expect(page.getByRole('button', { name: 'Sign out' })).toHaveCount(0);
  await page.getByRole('button', { name: 'Profile', exact: true }).click();
  await expect(page).toHaveURL(/\/profiles\/ada$/);
  await expect(page.getByText('Joined Jan 2021')).toBeVisible();
  await page.goto('/settings/account');
  await expect(page.getByRole('button', { name: 'Sign out' })).toBeVisible();
});

test('a fresh search context opens on Accounts after Posts was saved', async ({ browser }) => {
  const visit = async () => {
    const context = await browser.newContext();
    const page = await context.newPage();
    await page.setViewportSize({ width: 390, height: 844 });
    await install(page);
    await page.goto('/search');
    await page.getByRole('searchbox').fill('ada');
    await expect(page.getByRole('button', { name: '@ada' })).toBeVisible();
    await page.getByRole('tab', { name: 'Posts' }).click();
    await expect(page.getByRole('tab', { name: 'Posts' })).toHaveAttribute('aria-selected', 'true');
    await context.close();
  };
  await visit();
  await visit();
});

test('shots of the social surfaces', async ({ browser }) => {
  test.skip(!process.env.MEWO_SHOTS, 'set MEWO_SHOTS=1 to write social screenshots');
  const dir = process.env.MEWO_SHOTS_DIR ?? 'test-results/social-shots';
  await mkdir(dir, { recursive: true });
  const shots: { theme: 'x_light' | 'x_lights_out'; width: number }[] = [];
  for (const theme of ['x_light', 'x_lights_out'] as const) {
    for (const width of [390, 1280]) shots.push({ theme, width });
  }
  for (const { theme, width } of shots) {
    const label = `${width}-${theme === 'x_light' ? 'light' : 'dark'}`;
    const context = await browser.newContext({
      viewport: { width, height: width === 390 ? 844 : 900 },
    });
    const page = await context.newPage();
    try {
      await install(page, theme);
      await page.goto('/');
      await expect(page.getByText('byron reposted')).toBeVisible();
      await expect(page.getByText('Oct 3, 2024')).toBeVisible();
      await expect(page.getByRole('button', { name: 'Reposts, 1' }).first()).toBeEnabled();
      await page.screenshot({ path: `${dir}/web-home-feed-${label}.png` });
      await page.getByRole('tab', { name: 'Books' }).click();
      await expect(page.getByText('Emma', { exact: true })).toBeVisible();
      await page.screenshot({ path: `${dir}/web-home-books-${label}.png` });
      await page.getByRole('button', { name: 'Open account drawer' }).click();
      await expect(page.getByRole('button', { name: 'Profile', exact: true })).toBeVisible();
      await page.screenshot({ path: `${dir}/web-drawer-${label}.png` });
      await page.keyboard.press('Escape');
      await page.goto('/search');
      await page.getByRole('searchbox').fill('ada');
      await expect(page.getByRole('button', { name: '@ada' })).toBeVisible();
      await page.screenshot({ path: `${dir}/web-search-accounts-${label}.png` });
      await page.getByRole('tab', { name: 'Books' }).click();
      await expect(page.getByText('Emma', { exact: true })).toBeVisible();
      await page.screenshot({ path: `${dir}/web-search-books-${label}.png` });
      await page.getByRole('tab', { name: 'Posts' }).click();
      const repost = page.getByRole('button', { name: 'Reposts, 1' });
      await expect(repost).toBeEnabled();
      await repost.click();
      const mine = page.getByRole('button', { name: 'Reposts, 2' });
      await expect(mine).toBeEnabled();
      await expect(mine).toHaveAttribute('aria-pressed', 'true');
      await page.mouse.move(0, 0);
      await page.screenshot({ path: `${dir}/web-search-posts-${label}.png` });
      await page.screenshot({ path: `${dir}/web-repost-active-${label}.png` });
      await page.goto('/profiles/ada');
      await expect(page.getByText('Joined Jan 2021')).toBeVisible();
      await expect(page.getByText('Oct 3, 2024')).toBeVisible();
      await expect(page.getByRole('button', { name: 'Reposts, 2' })).toBeEnabled();
      await page.screenshot({ path: `${dir}/web-profile-posts-${label}.png` });
      await page.getByRole('tab', { name: 'Reposts' }).click();
      await expect(page.getByText('Emma Woodhouse, handsome, clever, and rich.')).toBeVisible();
      await page.screenshot({ path: `${dir}/web-profile-reposts-${label}.png` });
      await page.getByRole('tab', { name: 'Books' }).click();
      await expect(page.getByText('Emma', { exact: true })).toBeVisible();
      await page.screenshot({ path: `${dir}/web-profile-books-${label}.png` });
    } finally {
      await context.close();
    }
  }
});
