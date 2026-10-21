import { expect, test, type Page, type Route } from '@playwright/test';
import { readFileSync } from 'node:fs';
import { mkdir } from 'node:fs/promises';

const COVER = readFileSync(new URL('../fixtures/cover.jpg', import.meta.url));

function bookRequest(over: Record<string, unknown> = {}) {
  return {
    id: 'req-1',
    workKey: '/works/OL893415W',
    title: 'Dune',
    author: 'Frank Herbert',
    year: 1965,
    coverId: 11481354,
    state: 'downloading',
    progress: 42,
    error: null,
    bookId: null,
    requester: 'ada',
    createdAt: 1,
    updatedAt: 1,
    ...over,
  };
}

const supportedLanguages = [
  { code: 'en', name: 'English' },
  { code: 'es', name: 'Spanish' },
  { code: 'fr', name: 'French' },
  { code: 'de', name: 'German' },
  { code: 'it', name: 'Italian' },
  { code: 'pt', name: 'Portuguese' },
  { code: 'nl', name: 'Dutch' },
  { code: 'sv', name: 'Swedish' },
  { code: 'pl', name: 'Polish' },
  { code: 'ru', name: 'Russian' },
  { code: 'ja', name: 'Japanese' },
  { code: 'zh', name: 'Chinese' },
  { code: 'ko', name: 'Korean' },
];

function freshState() {
  return {
    canRequest: true,
    admin: true,
    settings: {
      allowSignup: true,
      maxEpubBytes: 64 * 1024 * 1024,
      requestsEnabled: true,
      prowlarrUrl: 'http://prowlarr.media.svc.cluster.local:9696',
      sabnzbdUrl: 'http://sabnzbd.media.svc.cluster.local:8080',
      sabnzbdCategory: 'books',
      languages: ['en'],
      supportedLanguages,
      downloadsDir: '/downloads',
      secrets: { prowlarrApiKey: true, sabnzbdApiKey: false },
    },
    requests: [bookRequest()],
    catalogRequest: null as ReturnType<typeof bookRequest> | null,
    catalog: null as null | {
      workKey: string;
      title: string;
      author: string;
      year: number | null;
      coverId: number | null;
      request: ReturnType<typeof bookRequest> | null;
    }[],
  };
}

async function install(
  page: Page,
  options: {
    canRequest?: boolean;
    admin?: boolean;
    theme?: 'x_light' | 'x_lights_out';
    requests?: ReturnType<typeof bookRequest>[];
    coverFails?: boolean;
  } = {},
) {
  const theme = options.theme ?? 'x_light';
  await page.addInitScript((nextTheme) => {
    localStorage.setItem('mewo.session', JSON.stringify({ token: 'token', username: 'ada' }));
    localStorage.setItem('mewo.display', JSON.stringify({
      theme: nextTheme, step: 2, mewo: false, keepBars: true,
    }));
  }, theme);
  const state = freshState();
  state.canRequest = options.canRequest ?? true;
  state.admin = options.admin ?? true;
  if (!state.canRequest) state.settings.requestsEnabled = false;
  if (options.requests) state.requests = options.requests;
  await page.route('**/covers.openlibrary.org/**', (route) => {
    if (options.coverFails) return route.fulfill({ status: 404, body: '' });
    return route.fulfill({ status: 200, contentType: 'image/jpeg', body: COVER });
  });
  await page.route('**/v1/**', (route) => answer(route, state));
  return state;
}

async function answer(route: Route, state: ReturnType<typeof freshState>) {
  const url = new URL(route.request().url());
  const path = url.pathname;
  const method = route.request().method();
  const json = (body: unknown, status = 200) => route.fulfill({
    status,
    contentType: 'application/json',
    body: JSON.stringify(body),
  });
  if (path === '/v1/me') {
    return json({ username: 'ada', isAdmin: state.admin, canRequest: state.canRequest });
  }
  if (path === '/v1/library') return json({ books: [] });
  if (path === '/v1/feed') return json({ items: [], nextCursor: null });
  if (path === '/v1/search') {
    const type = url.searchParams.get('type');
    const kind = type === 'books' || type === 'posts' || type === 'accounts' ? type : 'accounts';
    return json({ type: kind, items: [], nextCursor: null });
  }
  if (path === '/v1/catalog/search') {
    return json({
      items: state.catalog ?? [{
        workKey: '/works/OL893415W',
        title: 'Dune',
        author: 'Frank Herbert',
        year: 1965,
        coverId: 11481354,
        request: state.catalogRequest,
      }],
      nextPage: null,
    });
  }
  if (path === '/v1/requests' && method === 'POST') {
    state.catalogRequest = bookRequest({ id: 'req-new', state: 'requested', progress: 0 });
    return json(state.catalogRequest, 201);
  }
  if (path === '/v1/requests' && method === 'GET') return json({ items: state.requests });
  if (method === 'DELETE' && path.startsWith('/v1/requests/')) {
    const id = decodeURIComponent(path.slice('/v1/requests/'.length));
    const row = state.requests.find((item) => item.id === id);
    if (row && row.state !== 'imported') {
      row.state = 'canceled';
      row.error = null;
    }
    return route.fulfill({ status: 204, body: '' });
  }
  if (!state.admin && path.startsWith('/v1/admin/')) return json({ error: 'Admins only.' }, 403);
  if (path === '/v1/admin/settings' && method === 'PATCH') {
    const patch = route.request().postDataJSON() as Record<string, unknown>;
    if ('prowlarrApiKey' in patch) {
      state.settings.secrets.prowlarrApiKey = typeof patch.prowlarrApiKey === 'string' && patch.prowlarrApiKey.length > 0;
    }
    if ('sabnzbdApiKey' in patch) {
      state.settings.secrets.sabnzbdApiKey = typeof patch.sabnzbdApiKey === 'string' && patch.sabnzbdApiKey.length > 0;
    }
    for (const key of ['allowSignup', 'maxEpubBytes', 'requestsEnabled', 'prowlarrUrl', 'sabnzbdUrl', 'sabnzbdCategory', 'languages'] as const) {
      if (key in patch) (state.settings as Record<string, unknown>)[key] = patch[key];
    }
    return json(state.settings);
  }
  if (path === '/v1/admin/settings') return json(state.settings);
  if (path === '/v1/admin/server') {
    return json({ users: 2, books: 3, comments: 4, reposts: 1, storageBytes: 40_500_000 });
  }
  if (path === '/v1/admin/requests/test' && method === 'POST') {
    return json({
      checks: [
        { name: 'prowlarr', ok: true, detail: '2.4.0 · 3 usenet indexers with ebooks' },
        { name: 'sabnzbd', ok: true, detail: '4.3.2' },
        { name: 'downloads', ok: true, detail: '/downloads' },
      ],
    });
  }
  return json({});
}

test('the Catalog tab is hidden without canRequest', async ({ page }) => {
  await page.setViewportSize({ width: 390, height: 844 });
  await install(page, { canRequest: false });
  await page.goto('/search');
  await expect(page.getByRole('tab', { name: 'Accounts' })).toBeVisible();
  await expect(page.getByRole('tab', { name: 'Catalog' })).toHaveCount(0);
  await page.goto('/settings');
  await expect(page.getByRole('button', { name: 'Library' })).toBeVisible();
  await expect(page.getByRole('button', { name: 'Requests', exact: true })).toHaveCount(0);
});

test('a broken cover shows the title letter', async ({ page }) => {
  await page.setViewportSize({ width: 390, height: 844 });
  await install(page, { coverFails: true });
  await page.goto('/search');
  await page.getByRole('tab', { name: 'Catalog' }).click();
  await page.getByPlaceholder('Search').fill('dune');
  const cover = page.locator('.book-cover');
  await expect(cover).toBeVisible();
  await expect(cover).toHaveText('D');
  await expect(cover.locator('img')).toHaveCount(0);
});

test('Request flips the icon', async ({ page }) => {
  await page.setViewportSize({ width: 390, height: 844 });
  await install(page);
  await page.goto('/search');
  await page.getByRole('tab', { name: 'Catalog' }).click();
  await page.getByPlaceholder('Search').fill('dune');
  const row = page.getByRole('button', { name: 'Request' });
  await expect(row).toBeEnabled();
  await row.click();
  await expect(page.getByRole('button', { name: 'Searching', exact: true })).toBeDisabled();
  await expect(page.getByRole('button', { name: 'Request' })).toHaveCount(0);
});

test('the Requests page shows Downloading 42% and cancel works', async ({ page }) => {
  await page.setViewportSize({ width: 390, height: 844 });
  await install(page);
  await page.goto('/settings/requests');
  await expect(page.getByText('Downloading 42%')).toBeVisible();
  await page.getByRole('button', { name: 'Cancel' }).click();
  await expect(page.getByText('Canceled')).toBeVisible();
  await expect(page.getByRole('button', { name: 'Retry' })).toBeVisible();
});

test('book languages save through the settings patch and the last one stays on', async ({ page }) => {
  await page.setViewportSize({ width: 390, height: 844 });
  await install(page);
  await page.goto('/settings/server');
  await expect(page.getByText('Requests only download books in these languages.')).toBeVisible();
  const field = page.getByRole('button', { name: 'Book languages' });
  await expect(field).toHaveAttribute('aria-haspopup', 'listbox');
  await expect(field).toHaveAttribute('aria-expanded', 'false');
  await expect(field).toContainText('English');
  await field.click();
  const list = page.getByRole('listbox', { name: 'Book languages' });
  await expect(list).toBeVisible();
  await expect(field).toHaveAttribute('aria-expanded', 'true');
  const english = page.getByRole('option', { name: 'English' });
  const german = page.getByRole('option', { name: 'German' });
  await expect(english).toHaveAttribute('aria-selected', 'true');
  await expect(german).toHaveAttribute('aria-selected', 'false');
  await expect(english).toBeFocused();
  const add = page.waitForRequest((request) =>
    request.method() === 'PATCH' && request.url().includes('/v1/admin/settings'));
  await page.keyboard.press('ArrowDown');
  await page.keyboard.press('ArrowDown');
  await page.keyboard.press('ArrowDown');
  await expect(german).toBeFocused();
  await page.keyboard.press('Enter');
  expect((await add).postDataJSON()).toEqual({ languages: ['en', 'de'] });
  await expect(list).toBeVisible();
  await expect(english).toHaveAttribute('aria-selected', 'true');
  await expect(german).toHaveAttribute('aria-selected', 'true');
  await expect(field).toContainText('English, German');
  await page.keyboard.press('Escape');
  await expect(list).toHaveCount(0);
  await page.reload();
  await expect(field).toContainText('English, German');
  await field.click();
  await expect(german).toHaveAttribute('aria-selected', 'true');
  const remove = page.waitForRequest((request) =>
    request.method() === 'PATCH' && request.url().includes('/v1/admin/settings'));
  await german.click();
  expect((await remove).postDataJSON()).toEqual({ languages: ['en'] });
  await expect(list).toBeVisible();
  await expect(german).toHaveAttribute('aria-selected', 'false');
  let locked = false;
  page.on('request', (request) => {
    if (request.method() === 'PATCH') locked = true;
  });
  await english.click();
  await expect(english).toHaveAttribute('aria-selected', 'true');
  expect(locked).toBe(false);
});

test('a saved key shows Saved, blur sends the key, and test connection lists three checks', async ({ page }) => {
  await page.setViewportSize({ width: 1280, height: 900 });
  await install(page);
  await page.goto('/settings/server');
  const key = page.locator('label', { hasText: 'Prowlarr API key' }).locator('input');
  await expect(key).toHaveAttribute('placeholder', 'Saved');
  const patch = page.waitForRequest((request) =>
    request.method() === 'PATCH' && request.url().includes('/v1/admin/settings'));
  await key.fill('prowlarr-key-sentinel');
  await key.blur();
  expect((await patch).postDataJSON()).toEqual({ prowlarrApiKey: 'prowlarr-key-sentinel' });
  await page.getByRole('button', { name: 'Test connection' }).click();
  await expect(page.getByText('Prowlarr', { exact: true })).toBeVisible();
  await expect(page.getByText('2.4.0 · 3 usenet indexers with ebooks')).toBeVisible();
  await expect(page.getByText('SABnzbd', { exact: true })).toBeVisible();
  await expect(page.getByText('4.3.2')).toBeVisible();
  await expect(page.getByText('Downloads folder', { exact: true })).toHaveCount(2);
  await expect(page.getByText('/downloads').first()).toBeVisible();
});

test('shots of catalog, requests, and the book requests group', async ({ browser }) => {
  test.skip(!process.env.MEWO_SHOTS, 'set MEWO_SHOTS=1 to write request screenshots');
  const dir = process.env.MEWO_SHOTS_DIR ?? 'test-results/book-requests';
  await mkdir(dir, { recursive: true });
  const shots: { theme: 'x_light' | 'x_lights_out'; width: number }[] = [];
  for (const theme of ['x_light', 'x_lights_out'] as const) {
    for (const width of [1280, 390]) shots.push({ theme, width });
  }
  const shelf = [
    bookRequest({ id: 'req-wait', state: 'requested', progress: 0, error: 'No release yet.', title: 'Pride and Prejudice' }),
    bookRequest(),
    bookRequest({ id: 'req-fail', state: 'failed', progress: 0, error: 'No EPUB in the download.', title: 'Emma' }),
  ];
  for (const { theme, width } of shots) {
    const label = `${width}-${theme === 'x_light' ? 'light' : 'dark'}`;
    const context = await browser.newContext({
      viewport: { width, height: width === 390 ? 844 : 900 },
      colorScheme: theme === 'x_light' ? 'light' : 'dark',
    });
    const page = await context.newPage();
    try {
      const state = await install(page, { theme, requests: shelf });
      state.catalog = [
        {
          workKey: '/works/OL893415W',
          title: 'Dune',
          author: 'Frank Herbert',
          year: 1965,
          coverId: 11481354,
          request: null,
        },
        {
          workKey: '/works/OL45883W',
          title: 'Neuromancer',
          author: 'William Gibson',
          year: 1984,
          coverId: null,
          request: null,
        },
      ];
      await page.goto('/search');
      await page.getByRole('tab', { name: 'Catalog' }).click();
      await page.getByPlaceholder('Search').fill('dune');
      await expect(page.getByRole('button', { name: 'Request' }).first()).toBeVisible();
      await expect(page.locator('.book-cover img')).toBeVisible();
      await expect(page.locator('.book-cover').nth(1)).toHaveText('N');
      await expect(page.getByText('Neuromancer')).toBeVisible();
      await page.screenshot({ path: `${dir}/catalog-${label}.png` });
      await page.goto('/settings/requests');
      await expect(page.getByText('Downloading 42%')).toBeVisible();
      await page.screenshot({ path: `${dir}/requests-${label}.png` });
      await page.goto('/settings/server');
      const heading = page.getByRole('heading', { name: 'Book requests' });
      await expect(heading).toBeVisible();
      await heading.scrollIntoViewIfNeeded();
      await page.screenshot({ path: `${dir}/server-${label}.png`, fullPage: true });
    } finally {
      await context.close();
    }
  }
});
