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

const post = {
  id: '0',
  kind: 'Body',
  text: 'Emma Woodhouse, handsome, clever, and rich.',
  chapter: 'One',
};

const comment = {
  id: 'c1',
  username: 'ada',
  text: 'This opening still lands.',
  createdAt: 5,
  mine: true,
};

let reposted = false;

async function install(page: Page, theme: 'x_light' | 'x_lights_out' = 'x_light') {
  reposted = false;
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
  if (path === '/v1/library') return json({ books: [book, privateBook] });
  if (path === '/v1/feed') {
    return json({
      items: [
        {
          type: 'repost',
          actor: { username: 'byron' },
          book,
          post,
          at: 10,
        },
        {
          type: 'comment',
          actor: { username: 'ada' },
          book,
          post,
          comment,
          at: 9,
        },
      ],
      nextCursor: null,
    });
  }
  if (path === '/v1/search') {
    const type = url.searchParams.get('type');
    if (type === 'books') return json({ type: 'books', items: [book], nextCursor: null });
    if (type === 'posts') return json({ type: 'posts', items: [{ book, post }], nextCursor: null });
    return json({
      type: 'accounts',
      items: [{ username: 'ada', mine: true }, { username: 'byron', mine: false }],
      nextCursor: null,
    });
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
    return json({ items: [{ book, post, repostedAt: 6 }], nextCursor: null });
  }
  if (path === '/v1/users/ada/books') return json({ items: [book] });
  if (path === '/v1/users/nobody') return json({ error: 'Not found.' }, 404);
  if (path === '/v1/books/b1/comments') {
    return json({
      counts: { '0': 1 },
      mine: ['0'],
      reposts: reposted ? { '0': 1 } : {},
      reposted: reposted ? ['0'] : [],
    });
  }
  if (path === '/v1/books/b1/likes') return json({ ids: [] });
  if (path === '/v1/books/b1/posts') return json({ posts: [post] });
  if (path === '/v1/books/b1/posts/0/repost') {
    reposted = method === 'PUT';
    return json({ reposts: reposted ? 1 : 0, reposted });
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
  const repost = page.getByRole('button', { name: 'Repost', exact: true });
  await expect(repost).toBeVisible();
  await repost.click();
  await expect(page.getByRole('button', { name: 'Reposts, 1' })).toHaveAttribute('aria-pressed', 'true');
  await page.reload();
  await page.getByRole('searchbox').fill('line');
  await expect(page.getByRole('button', { name: 'Reposts, 1' })).toHaveAttribute('aria-pressed', 'true');
});

test('home opens on Feed and Books keeps the shelf', async ({ page }) => {
  await page.setViewportSize({ width: 390, height: 844 });
  await install(page);
  await page.goto('/');
  await expect(page.getByRole('tab', { name: 'Feed' })).toHaveAttribute('aria-selected', 'true');
  await expect(page.getByText('byron reposted')).toBeVisible();
  await expect(page.getByText('This opening still lands.')).toBeVisible();
  await page.getByRole('tab', { name: 'Books' }).click();
  await expect(page).toHaveURL(/tab=books/);
  await expect(page.getByText('Emma', { exact: true })).toBeVisible();
  await expect(page.getByText('Persuasion')).toBeVisible();
});

test('the drawer Profile row opens your profile', async ({ page }) => {
  await page.setViewportSize({ width: 390, height: 844 });
  await install(page);
  await page.goto('/');
  await page.getByRole('button', { name: 'Open account drawer' }).click();
  await page.getByRole('button', { name: 'Profile', exact: true }).click();
  await expect(page).toHaveURL(/\/profiles\/ada$/);
  await expect(page.getByText('Joined Jan 2021')).toBeVisible();
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
      await expect(page.getByRole('button', { name: 'Repost', exact: true })).toBeVisible();
      await page.getByRole('button', { name: 'Repost', exact: true }).click();
      await expect(page.getByRole('button', { name: 'Reposts, 1' })).toBeVisible();
      await page.screenshot({ path: `${dir}/web-search-posts-${label}.png` });
      await page.screenshot({ path: `${dir}/web-repost-active-${label}.png` });
      await page.goto('/profiles/ada');
      await expect(page.getByText('Joined Jan 2021')).toBeVisible();
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
