import { expect, test, type Page, type Route } from '@playwright/test';
import { readFileSync } from 'node:fs';
import { mkdir } from 'node:fs/promises';

const COVER = readFileSync(new URL('../fixtures/cover.jpg', import.meta.url));

async function expectSection(page: Page, title: string): Promise<void> {
  const header = page.locator('header.search-section');
  await expect(header).toBeVisible();
  await expect(header).toHaveText(title);
}

async function shot(page: Page, name: string): Promise<void> {
  if (!process.env.MEWO_SHOTS) return;
  const dir = process.env.MEWO_SHOTS_DIR ?? 'test-results/search-shots';
  await mkdir(dir, { recursive: true });
  await page.screenshot({ path: `${dir}/${name}.png` });
}

function book(id: string, title: string, author: string, over: Record<string, unknown> = {}) {
  return {
    id,
    title,
    author,
    handle: author.toLowerCase().replace(/[^a-z]+/g, '') || 'book',
    importedAt: 1,
    postCount: 4,
    progressIndex: 0,
    isSample: false,
    mine: true,
    shared: false,
    ownerUsername: 'ada',
    held: false,
    ...over,
  };
}

const books = [
  book('b1', 'Emma', 'Austen'),
  book('b2', 'Pride and Prejudice', 'Austen'),
  book('b3', 'Persuasion', 'Austen'),
  book('b4', 'Dune', 'Herbert', { mine: false, shared: true, ownerUsername: 'grace' }),
  book('b5', 'Neuromancer', 'Gibson'),
  book('b6', 'Frankenstein', 'Shelley'),
  book('b7', 'Kindred', 'Butler'),
  book('b8', 'The Dispossessed', 'Le Guin'),
  book('b9', 'The Left Hand of Darkness', 'Le Guin'),
  book('b10', 'Foundation', 'Asimov'),
  book('b11', 'Parable of the Sower', 'Butler'),
  book('b12', 'Orlando', 'Woolf'),
];

const posts = [
  {
    book: book('b1', 'Emma', 'Austen'),
    post: { id: '1', kind: 'Body', text: 'Emma Woodhouse, handsome, clever, and rich.', chapter: 'One' },
  },
  {
    book: book('b4', 'Dune', 'Herbert', { mine: false, shared: true, ownerUsername: 'grace' }),
    post: { id: '2', kind: 'Body', text: 'A beginning is the time for taking the most delicate care.', chapter: '1' },
  },
];

function work(
  olid: string,
  title: string,
  author: string,
  year: number,
  over: Record<string, unknown> = {},
) {
  return {
    workKey: `/works/${olid}`,
    title,
    author,
    year,
    coverId: 11481354,
    request: null,
    onServer: null,
    ...over,
  };
}

const trending = [
  work('OL893415W', 'Dune', 'Frank Herbert', 1965, { coverId: 11481354 }),
  work('OL45883W', 'Neuromancer', 'William Gibson', 1984, {
    coverId: null,
    onServer: {
      bookId: 'b-neuro',
      ownerUsername: 'grace',
      mine: false,
      shared: true,
      held: false,
    },
  }),
  work('OL27563W', 'Foundation', 'Isaac Asimov', 1951, {
    onServer: {
      bookId: 'b-foundation',
      ownerUsername: 'secret',
      mine: false,
      shared: false,
      held: true,
    },
  }),
  work('OL893416W', 'The Left Hand of Darkness', 'Ursula K. Le Guin', 1969),
  work('OL893417W', 'Kindred', 'Octavia Butler', 1979),
  work('OL893418W', 'Frankenstein', 'Mary Shelley', 1818),
  work('OL893419W', 'Orlando', 'Virginia Woolf', 1928),
  work('OL893420W', 'Persuasion', 'Jane Austen', 1817),
  work('OL893421W', 'Parable of the Sower', 'Octavia Butler', 1993),
  work('OL893422W', 'The Dispossessed', 'Ursula K. Le Guin', 1974),
];

function detailFor(stage: string, info: boolean) {
  const base = {
    id: 'req-1',
    workKey: '/works/OL893415W',
    title: 'Dune',
    author: 'Frank Herbert',
    year: 1965,
    coverId: 11481354,
    state: 'requested',
    progress: 0,
    error: null as string | null,
    bookId: null as string | null,
    requester: 'ada',
    createdAt: Date.parse('2026-10-21T15:00:00Z'),
    updatedAt: Date.parse('2026-10-21T15:00:00Z'),
    attempts: 1,
    maxAttempts: 3,
    nextCheckAt: null as number | null,
    releaseTitle: null as string | null,
    releaseSize: null as number | null,
    phase: 'searching',
  };
  let request: typeof base | null = null;
  let onServer = null;
  if (stage === 'searching') request = { ...base, phase: 'searching' };
  if (stage === 'downloading') {
    request = {
      ...base,
      state: 'downloading',
      progress: 40,
      phase: 'downloading',
      releaseTitle: 'Dune - Frank Herbert.epub',
      releaseSize: 1_400_000,
    };
  }
  if (stage === 'failed') {
    request = {
      ...base,
      state: 'failed',
      phase: 'failed',
      error: 'No EPUB in the download.',
      attempts: 2,
    };
  }
  if (stage === 'imported') {
    request = {
      ...base,
      state: 'imported',
      progress: 100,
      phase: 'imported',
      bookId: 'book-dune',
      attempts: 1,
    };
    onServer = {
      bookId: 'book-dune',
      ownerUsername: 'ada',
      mine: true,
      shared: false,
      held: false,
    };
  }
  if (stage === 'canceled-copy') {
    request = { ...base, state: 'canceled', phase: 'canceled', error: null };
    onServer = {
      bookId: 'book-dune',
      ownerUsername: 'ada',
      mine: true,
      shared: false,
      held: false,
    };
  }
  if (stage === 'imported-empty') {
    request = {
      ...base,
      state: 'imported',
      progress: 100,
      phase: 'imported',
      bookId: null,
    };
  }
  if (stage === 'canceled') {
    request = { ...base, state: 'canceled', phase: 'canceled', error: null };
  }
  return {
    workKey: '/works/OL893415W',
    info: info ? {
      canonicalKey: '/works/OL893415W',
      title: 'Dune',
      subtitle: null,
      authors: ['Frank Herbert'],
      year: 1965,
      coverId: 11481354,
      description: 'Set on the desert planet Arrakis, Dune is the story of Paul Atreides. '
        + 'The spice must flow, and the great houses fight for control of it.',
      subjects: ['Science fiction', 'Arrakis', 'Deserts'],
      awards: ['Hugo Award 1966'],
      series: ['Dune'],
      editionCount: 155,
      pagesMedian: 608,
      languages: ['English', 'German', 'French'],
      hasAcceptedEdition: true,
      rating: { average: 4.3, count: 448 },
    } : null,
    infoError: info ? null : 'Open Library is not answering.',
    status: {
      request,
      onServer,
      active: stage === 'searching' || stage === 'downloading' ? 1 : 0,
      cap: 5,
      otherRequests: stage === 'downloading' ? [{
        id: 'req-lin',
        workKey: '/works/OL893415W',
        title: 'Dune',
        author: 'Frank Herbert',
        year: 1965,
        coverId: 11481354,
        state: 'downloading',
        progress: 40,
        error: null,
        bookId: null,
        requester: 'lin',
        createdAt: Date.parse('2026-10-21T14:00:00Z'),
        updatedAt: Date.parse('2026-10-21T14:00:00Z'),
      }] : [],
    },
  };
}

const shelfBook = book('book-dune', 'Dune', 'Frank Herbert');

async function install(
  page: Page,
  options: {
    kind?: string;
    holdBooks?: boolean;
    accounts?: { username: string; mine: boolean }[];
    emptyQuery?: string;
    info?: boolean;
    theme?: 'x_light' | 'x_lights_out';
  } = {},
) {
  const state = {
    stage: 'none',
    holdBooks: options.holdBooks ?? false,
    releaseBooks: () => {},
    booksGate: Promise.resolve(),
    bookFetches: 0,
    info: options.info ?? true,
    accounts: options.accounts ?? [
      { username: 'grace', mine: false },
      { username: 'lin', mine: false },
      { username: 'noor', mine: false },
    ],
    miss: options.emptyQuery ?? null as string | null,
    catalogQueries: [] as string[],
    catalogFailure: null as null | { query: string; status: number; error: string },
    holdDetail: false,
    releaseDetail: () => {},
    retryTo: null as string | null,
    items: null as null | ReturnType<typeof work>[],
    emptyBrowse: false,
    holdCatalog: false,
    releaseCatalog: () => {},
    requestPosts: [] as string[],
    details: null as null | Record<string, unknown>,
    shelf: false,
  };
  state.booksGate = new Promise((resolve) => { state.releaseBooks = resolve; });
  await page.addInitScript(({ kind, theme }) => {
    localStorage.setItem('mewo.session', JSON.stringify({ token: 'token', username: 'ada' }));
    localStorage.setItem('mewo.display', JSON.stringify({
      theme, step: 2, mewo: false, keepBars: true,
    }));
    if (kind) localStorage.setItem('mewo.search.kind', JSON.stringify(kind));
  }, { kind: options.kind ?? 'books', theme: options.theme ?? 'x_light' });
  await page.route('**/covers.openlibrary.org/**', (route) => route.fulfill({
    status: 200, contentType: 'image/jpeg', body: COVER,
  }));
  await page.route('**/v1/**', (route) => answer(route, state));
  return state;
}

async function answer(route: Route, state: Awaited<ReturnType<typeof install>>) {
  const url = new URL(route.request().url());
  const path = url.pathname;
  const method = route.request().method();
  const json = (body: unknown, status = 200) => route.fulfill({
    status,
    contentType: 'application/json',
    body: JSON.stringify(body),
  });
  if (method === 'POST' && path.startsWith('/v1/requests')) state.requestPosts.push(path);
  if (path === '/v1/me') return json({ username: 'ada', isAdmin: true, canRequest: true });
  if (path === '/v1/library') {
    if (state.shelf) return json({ books: [book('b1', 'Emma', 'Austen')] });
    return json({ books: state.stage === 'imported' ? [shelfBook] : [] });
  }
  if (path === '/v1/feed') return json({ items: [], nextCursor: null });
  if (path === '/v1/search') {
    const type = url.searchParams.get('type') ?? 'accounts';
    const q = url.searchParams.get('q') ?? '';
    if (type === 'books') {
      state.bookFetches += 1;
      if (state.holdBooks) {
        state.holdBooks = false;
        await state.booksGate;
      }
    }
    if (state.miss && q === state.miss) return json({ type, items: [], nextCursor: null });
    if (type === 'books') return json({ type, items: books, nextCursor: null });
    if (type === 'posts') return json({ type, items: posts, nextCursor: null });
    return json({ type: 'accounts', items: state.accounts, nextCursor: null });
  }
  if (path === '/v1/catalog/search') {
    const q = url.searchParams.get('q') ?? '';
    state.catalogQueries.push(q);
    if (state.holdCatalog) {
      state.holdCatalog = false;
      await new Promise<void>((resolve) => { state.releaseCatalog = resolve; });
    }
    const failure = state.catalogFailure;
    if (failure && q === failure.query) return json({ error: failure.error }, failure.status);
    if (state.emptyBrowse && q === '') return json({ items: [], nextPage: null });
    if (state.miss && q === state.miss) return json({ items: [], nextPage: null });
    return json({ items: state.items ?? trending, nextPage: null });
  }
  if (path.startsWith('/v1/catalog/works/')) {
    const olid = decodeURIComponent(path.slice('/v1/catalog/works/'.length));
    if (state.details?.[olid]) return json(state.details[olid]);
    if (olid !== 'OL893415W') return json({});
  }
  if (path === '/v1/catalog/works/OL893415W') {
    if (state.holdDetail) {
      state.holdDetail = false;
      await new Promise<void>((resolve) => { state.releaseDetail = resolve; });
    }
    return json(detailFor(state.stage, state.info));
  }
  if (path === '/v1/requests' && method === 'POST') {
    const retry = path.endsWith('/retry');
    state.stage = retry ? state.stage : 'searching';
    return json(detailFor(state.stage, state.info).status.request, 201);
  }
  if (method === 'POST' && path === '/v1/requests/req-1/retry') {
    state.stage = state.retryTo ?? 'imported';
    return json(detailFor(state.stage, state.info).status.request);
  }
  if (path.startsWith('/v1/books/book-dune/posts')) {
    return json({
      posts: [{
        id: '0',
        kind: 'Body',
        text: 'A beginning is the time for taking the most delicate care.',
        chapter: '1',
      }],
    });
  }
  if (path === '/v1/books/book-dune') return json(shelfBook);
  if (path.endsWith('/likes')) return json({ ids: [] });
  if (path.endsWith('/comments')) return json({ counts: {}, mine: [], reposts: {}, reposted: [] });
  if (path.endsWith('/cover')) return route.fulfill({ status: 404, body: '' });
  return json({});
}

test('empty search shows a browse page, keeps each tab, and restores scroll', async ({ page }) => {
  await page.setViewportSize({ width: 390, height: 844 });
  const state = await install(page, { kind: 'books', holdBooks: true });
  await page.goto('/search');
  await expect(page.locator('[aria-busy="true"]')).toBeVisible();
  state.releaseBooks();
  await expectSection(page, 'Popular on this server');
  await expect(page.getByText('Emma').first()).toBeVisible();
  await shot(page, 'search-books');

  const main = page.locator('main');
  await main.evaluate((el) => { el.scrollTop = 160; });
  const booksScroll = await main.evaluate((el) => el.scrollTop);
  expect(booksScroll).toBeGreaterThan(80);
  const fetches = state.bookFetches;

  await page.getByRole('tab', { name: 'Accounts' }).click();
  await expectSection(page, 'Suggested accounts');
  await expect(page.getByText('@grace')).toBeVisible();
  await shot(page, 'search-accounts');

  await page.getByRole('tab', { name: 'Books' }).click();
  await expect(page.getByText('Emma').first()).toBeVisible();
  expect(state.bookFetches).toBe(fetches);
  expect(await main.evaluate((el) => el.scrollTop)).toBe(booksScroll);

  await page.getByRole('tab', { name: 'Posts' }).click();
  await expectSection(page, 'Top posts');
  await expect(page.getByText('Emma Woodhouse, handsome, clever, and rich.')).toBeVisible();
  await shot(page, 'search-posts');

  await page.getByRole('tab', { name: 'Catalog' }).click();
  await expectSection(page, 'Trending on Open Library');
  const held = page.getByRole('button', { name: 'Foundation, Isaac Asimov, 1951, On your shelf' });
  await expect(held).toBeVisible();
  await expect(held).not.toContainText('secret');
  await expect(page.getByRole('button', {
    name: 'Neuromancer, William Gibson, 1984, On server · @grace',
  })).toBeVisible();
  await shot(page, 'search-catalog');

  await main.evaluate((el) => { el.scrollTop = 140; });
  const catalogScroll = await main.evaluate((el) => el.scrollTop);
  expect(catalogScroll).toBeGreaterThan(80);
  await page.getByRole('tab', { name: 'Books' }).click();
  await expect(page.getByRole('heading', { name: 'Popular on this server' })).toBeVisible();
  expect(await main.evaluate((el) => el.scrollTop)).toBe(booksScroll);
  await page.getByRole('tab', { name: 'Catalog' }).click();
  await expect(page.getByText('Dune').first()).toBeVisible();
  expect(await main.evaluate((el) => el.scrollTop)).toBe(catalogScroll);
});

test('Search the Catalog keeps the query that missed', async ({ page }) => {
  await page.setViewportSize({ width: 390, height: 844 });
  await install(page, { kind: 'books', emptyQuery: 'zzzz' });
  await page.goto('/search');
  await page.getByPlaceholder('Search').fill('zzzz');
  await expect(page.getByRole('heading', { name: 'No books match “zzzz”' })).toBeVisible();
  await page.getByRole('button', { name: 'Search the Catalog' }).click();
  await expect(page.getByRole('tab', { name: 'Catalog' })).toHaveAttribute('aria-selected', 'true');
  await expect(page.getByPlaceholder('Search')).toHaveValue('zzzz');
  await expect(page.getByRole('heading', {
    name: 'Nothing in Open Library matches “zzzz”. Try the author\'s name or fewer words.',
  })).toBeVisible();
});

test('a catalog row opens a sheet and follows the request through to the book', async ({ page }) => {
  test.setTimeout(60_000);
  await page.setViewportSize({ width: 390, height: 844 });
  const state = await install(page, { kind: 'catalog' });
  await page.goto('/search');
  await expect(page.getByRole('heading', { name: 'Trending on Open Library' })).toBeVisible();
  const main = page.locator('main');
  await main.evaluate((el) => { el.scrollTop = 160; });
  const scroll = await main.evaluate((el) => el.scrollTop);

  // Click in the page. Playwright's click would scroll this first row into view first.
  await page.getByRole('button', { name: 'Dune, Frank Herbert, 1965' }).evaluate((el: HTMLElement) => el.click());
  const sheet = page.getByRole('dialog', { name: 'Dune' });
  await expect(sheet.getByRole('heading', { name: 'Dune' })).toHaveCount(1);
  await expect(sheet.locator('.hero')).toContainText('Frank Herbert · 1965');
  await expect(sheet.locator('.hero .dot')).toHaveCount(0);
  await expect(sheet.getByText('Science fiction · Arrakis · Deserts')).toBeVisible();
  await expect(sheet.getByRole('button', { name: 'Science fiction' })).toHaveCount(0);
  await expect(sheet.getByText('First published 1965 · ~608 pages · ★ 4.3')).toBeVisible();
  await expect(sheet.getByText('155 editions')).toHaveCount(0);
  await expect(sheet.getByText(/Editions in/)).toHaveCount(0);
  await expect(sheet.locator('.row').filter({ hasText: 'Series' })).toContainText('Dune');
  await expect(sheet.locator('.row').filter({ hasText: 'Awards' })).toContainText('Hugo Award 1966');
  await expect(sheet.getByRole('heading', { name: 'Awards' })).toHaveCount(0);
  const openLibrary = sheet.getByRole('link', { name: 'View on Open Library' });
  await expect(openLibrary).toHaveAttribute('href', 'https://openlibrary.org/works/OL893415W');
  await expect(openLibrary.locator('svg')).toBeVisible();
  await sheet.getByRole('button', { name: 'Close' }).click();
  await expect(sheet).toHaveCount(0);
  expect(await main.evaluate((el) => el.scrollTop)).toBe(scroll);

  await page.getByRole('button', { name: 'Dune, Frank Herbert, 1965' }).evaluate((el: HTMLElement) => el.click());
  const open = page.getByRole('dialog', { name: 'Dune' });
  await page.clock.install({ time: new Date('2026-10-21T15:00:00Z') });
  await open.getByRole('button', { name: 'Request', exact: true }).click();
  await expect(open.getByText('Looking for a release…')).toBeVisible();
  await expect(open.locator('.hero')).not.toContainText('Searching');
  const cancel = open.getByRole('button', { name: 'Cancel request' });
  await expect(cancel).toBeVisible();
  await expect(cancel).toHaveClass(/secondary/);
  await expect(open.getByText(/^Requested/)).toHaveCount(0);
  await shot(page, 'catalog-sheet-requested');

  state.stage = 'downloading';
  await page.clock.runFor(10_000);
  await expect(open.getByRole('progressbar', { name: 'Downloading 40%' })).toBeVisible();
  await expect(open.getByText('Dune - Frank Herbert.epub')).toHaveCount(0);
  await expect(open.getByText('Also requested by @lin')).toBeVisible();
  await expect(open.getByText(/@lin ·/)).toHaveCount(0);
  await shot(page, 'catalog-sheet-downloading');

  state.stage = 'failed';
  await page.clock.runFor(10_000);
  await expect(open.getByRole('button', { name: 'Retry' })).toBeVisible();
  await expect(open.getByText('No EPUB in the download.')).toBeVisible();
  await expect(open.getByText('Tried 2 of 3 releases')).toHaveCount(0);

  const library = page.waitForResponse((response) =>
    response.url().includes('/v1/library') && response.request().method() === 'GET');
  await open.getByRole('button', { name: 'Retry' }).click();
  await library;
  await expect(open.getByRole('button', { name: 'Open book' })).toBeVisible();
  await expect(open.getByText('On your shelf')).toHaveCount(1);
  await shot(page, 'catalog-sheet-imported');
  await open.getByRole('button', { name: 'Open book' }).click();
  await expect(page).toHaveURL(/\/books\/book-dune/);
  await expect(page.getByText('A beginning is the time for taking the most delicate care.')).toBeVisible();
  await page.goBack();
  await expect(page.getByRole('heading', { name: 'Trending on Open Library' })).toBeVisible();
  expect(await main.evaluate((el) => el.scrollTop)).toBe(scroll);
});

test('a canceled request with a server copy opens the book', async ({ page }) => {
  await page.setViewportSize({ width: 390, height: 844 });
  const state = await install(page, { kind: 'catalog' });
  state.stage = 'canceled-copy';
  await page.goto('/search');
  await page.getByRole('button', { name: 'Dune, Frank Herbert, 1965' }).click();
  const sheet = page.getByRole('dialog', { name: 'Dune' });
  await expect(sheet.getByRole('button', { name: 'Open book' })).toBeVisible();
  await expect(sheet.getByText('On your shelf')).toHaveCount(1);
  await expect(sheet.getByRole('button', { name: 'Request again' })).toHaveCount(0);
});

test('retry updates the sheet before the next detail load', async ({ page }) => {
  await page.setViewportSize({ width: 390, height: 844 });
  const state = await install(page, { kind: 'catalog' });
  state.stage = 'failed';
  await page.goto('/search');
  await page.getByRole('button', { name: 'Dune, Frank Herbert, 1965' }).click();
  const sheet = page.getByRole('dialog', { name: 'Dune' });
  await expect(sheet.getByRole('button', { name: 'Retry' })).toBeVisible();
  state.holdDetail = true;
  state.retryTo = 'searching';
  await sheet.getByRole('button', { name: 'Retry' }).click();
  await expect(sheet.getByText('Looking for a release…')).toBeVisible();
  await expect(sheet.getByRole('button', { name: 'Retry' })).toHaveCount(0);
  state.releaseDetail();
});

test('one or two characters on Catalog keep trending', async ({ page }) => {
  await page.setViewportSize({ width: 390, height: 844 });
  const state = await install(page, { kind: 'catalog' });
  await page.goto('/search');
  await expect(page.getByRole('heading', { name: 'Trending on Open Library' })).toBeVisible();
  await expect(page.getByRole('button', { name: 'Dune, Frank Herbert, 1965' })).toBeVisible();
  const before = state.catalogQueries.length;

  await page.getByPlaceholder('Search').fill('d');
  await page.waitForTimeout(500);
  await page.getByPlaceholder('Search').fill('du');
  await page.waitForTimeout(500);
  expect(state.catalogQueries.slice(before)).toEqual([]);
  await expect(page.getByRole('alert')).toHaveCount(0);
  await expect(page.getByText('Type at least')).toHaveCount(0);
  await expect(page.getByRole('heading', { name: 'Trending on Open Library' })).toBeVisible();
  await expect(page.getByRole('button', { name: 'Dune, Frank Herbert, 1965' })).toBeVisible();

  await page.getByPlaceholder('Search').fill('dun');
  await expect.poll(() => state.catalogQueries.includes('dun')).toBe(true);
  expect(state.catalogQueries.includes('d')).toBe(false);
  expect(state.catalogQueries.includes('du')).toBe(false);
});

test('a catalog search error replaces the list', async ({ page }) => {
  await page.setViewportSize({ width: 390, height: 844 });
  const state = await install(page, { kind: 'catalog' });
  state.catalogFailure = {
    query: 'dune',
    status: 502,
    error: "Couldn't search the catalog right now.",
  };
  await page.goto('/search');
  await expect(page.getByRole('button', { name: 'Dune, Frank Herbert, 1965' })).toBeVisible();

  await page.getByPlaceholder('Search').fill('dune');
  const alert = page.getByRole('alert');
  await expect(alert).toHaveText("Couldn't search the catalog right now.");
  await expect(page.getByRole('button', { name: 'Try again' })).toBeVisible();
  await expect(page.getByRole('heading', { name: 'Trending on Open Library' })).toHaveCount(0);
  await expect(page.getByRole('button', { name: 'Dune, Frank Herbert, 1965' })).toHaveCount(0);

  state.catalogFailure = {
    query: 'dune',
    status: 502,
    error: 'Open Library is not answering. Try again.',
  };
  await page.getByRole('button', { name: 'Try again' }).click();
  await expect(alert).toHaveText('Open Library is not answering.');
  await expect(page.getByRole('button', { name: 'Dune, Frank Herbert, 1965' })).toHaveCount(0);

  state.catalogFailure = null;
  await page.getByRole('button', { name: 'Try again' }).click();
  await expect(alert).toHaveCount(0);
  await expect(page.getByRole('button', { name: 'Dune, Frank Herbert, 1965' })).toBeVisible();
});

test('an Open Library miss keeps the row and still requests', async ({ page }) => {
  await page.setViewportSize({ width: 390, height: 844 });
  const state = await install(page, { kind: 'catalog', info: false });
  await page.goto('/search');
  await page.getByRole('button', { name: 'Dune, Frank Herbert, 1965' }).click();
  const sheet = page.getByRole('dialog', { name: 'Dune' });
  await expect(sheet.locator('.hero')).toContainText('Dune');
  await expect(sheet.getByText("Couldn't load details from Open Library.")).toBeVisible();
  await sheet.getByRole('button', { name: 'Request', exact: true }).click();
  await expect(sheet.getByText('Looking for a release…')).toBeVisible();
  expect(state.stage).toBe('searching');
});

test('an imported book with no copy is requested again', async ({ page }) => {
  await page.setViewportSize({ width: 390, height: 844 });
  const state = await install(page, { kind: 'catalog' });
  state.stage = 'imported-empty';
  state.items = [work('OL893415W', 'Dune', 'Frank Herbert', 1965, {
    request: {
      id: 'req-1',
      workKey: '/works/OL893415W',
      title: 'Dune',
      author: 'Frank Herbert',
      year: 1965,
      coverId: 11481354,
      state: 'imported',
      progress: 100,
      error: null,
      bookId: null,
      requester: 'ada',
      createdAt: 1,
      updatedAt: 1,
    },
  })];
  await page.goto('/search');
  await page.getByRole('button', { name: 'Request again', exact: true }).click();
  await expect.poll(() => state.requestPosts.slice()).toEqual(['/v1/requests']);

  state.requestPosts.length = 0;
  state.stage = 'imported-empty';
  await page.reload();
  await page.getByRole('button', { name: 'Dune, Frank Herbert, 1965', exact: true }).click();
  const sheet = page.getByRole('dialog', { name: 'Dune' });
  await expect(sheet.getByText('On your shelf')).toHaveCount(0);
  await expect(sheet.getByRole('button', { name: 'Open book' })).toHaveCount(0);
  await expect(sheet.getByRole('heading', { name: 'Dune' })).toHaveCount(1);
  await sheet.getByRole('button', { name: 'Request again', exact: true }).click();
  await expect.poll(() => state.requestPosts.slice()).toEqual(['/v1/requests']);
});

test('a live catalog row has no trailing control', async ({ page }) => {
  await page.setViewportSize({ width: 390, height: 844 });
  const state = await install(page, { kind: 'catalog' });
  state.items = [work('OL893415W', 'Dune', 'Frank Herbert', 1965, {
    request: {
      id: 'req-1',
      workKey: '/works/OL893415W',
      title: 'Dune',
      author: 'Frank Herbert',
      year: 1965,
      coverId: 11481354,
      state: 'requested',
      progress: 0,
      error: null,
      bookId: null,
      requester: 'ada',
      createdAt: 1,
      updatedAt: 1,
    },
  })];
  await page.goto('/search');
  await expect(page.getByRole('button', { name: 'Dune, Frank Herbert, 1965, Searching' })).toBeVisible();
  await expect(page.locator('.book-row.live')).toHaveCount(1);
  await expect(page.locator('.book-row > .row-action')).toHaveCount(0);
  await expect(page.getByRole('button', { name: 'Searching', exact: true })).toHaveCount(0);
});

test('the trending header hides when the list is empty or failed', async ({ page }) => {
  await page.setViewportSize({ width: 390, height: 844 });
  const state = await install(page, { kind: 'catalog' });
  await page.goto('/search');
  await page.getByPlaceholder('Search').fill('du');
  await expect(page.getByRole('heading', { name: 'Trending on Open Library' })).toBeVisible();

  state.emptyBrowse = true;
  await page.reload();
  await expect(page.getByRole('heading', { name: /No trending books/ })).toBeVisible();
  await expect(page.getByRole('heading', { name: 'Trending on Open Library' })).toHaveCount(0);

  state.emptyBrowse = false;
  state.catalogFailure = {
    query: '',
    status: 502,
    error: "Couldn't search the catalog right now.",
  };
  await page.reload();
  await expect(page.getByRole('alert')).toHaveText("Couldn't search the catalog right now.");
  await expect(page.getByRole('heading', { name: 'Trending on Open Library' })).toHaveCount(0);
  await expect(page.getByRole('button', { name: 'Try again' })).toBeVisible();
});

test('Browse catalog opens the Catalog tab', async ({ page }) => {
  await page.setViewportSize({ width: 390, height: 844 });
  await install(page, { kind: 'accounts' });
  await page.goto('/search');
  await expect(page.getByRole('tab', { name: 'Accounts' })).toHaveAttribute('aria-selected', 'true');
  await page.getByPlaceholder('Search').fill('du');
  await page.getByRole('link', { name: 'Home' }).click();
  await page.getByRole('tab', { name: 'Books' }).click();
  const add = page.getByRole('button', { name: 'Add a book', exact: true });
  await add.click();
  const menu = page.getByRole('menu');
  await expect(menu.getByRole('menuitem')).toHaveText(['Upload EPUB', 'Browse catalog']);
  await page.keyboard.press('Escape');
  await expect(menu).toHaveCount(0);
  await add.click();
  await page.getByRole('banner').click();
  await expect(menu).toHaveCount(0);
  await add.click();
  await menu.getByRole('menuitem', { name: 'Browse catalog' }).click();
  await expect(page).toHaveURL(/\/search$/);
  await expect(page.getByRole('tab', { name: 'Catalog' })).toHaveAttribute('aria-selected', 'true');
  await expect(page.getByPlaceholder('Search')).toHaveValue('du');
  await expect(page.getByRole('heading', { name: 'Trending on Open Library' })).toBeVisible();

  // Search is already mounted. Switching away and browsing again must select Catalog.
  await page.getByRole('tab', { name: 'Accounts' }).click();
  await expect(page.getByRole('tab', { name: 'Accounts' })).toHaveAttribute('aria-selected', 'true');
  await page.getByRole('link', { name: 'Home' }).click();
  await page.getByRole('tab', { name: 'Books' }).click();
  await page.getByRole('button', { name: 'Add a book', exact: true }).click();
  await page.getByRole('menuitem', { name: 'Browse catalog' }).click();
  await expect(page.getByRole('tab', { name: 'Catalog' })).toHaveAttribute('aria-selected', 'true');
  await expect(page.getByPlaceholder('Search')).toHaveValue('du');
});

function parityRequest(id: string, olid: string, title: string, over: Record<string, unknown> = {}) {
  return {
    id,
    workKey: `/works/${olid}`,
    title,
    author: 'Frank Herbert',
    year: 1965,
    coverId: null,
    state: 'requested',
    progress: 0,
    error: null as string | null,
    bookId: null as string | null,
    requester: 'ada',
    createdAt: 1,
    updatedAt: 1,
    ...over,
  };
}

function parityDetail(item: ReturnType<typeof work>, phase: string | null) {
  const request = item.request ? {
    ...item.request,
    attempts: 1,
    maxAttempts: 3,
    nextCheckAt: null,
    releaseTitle: null,
    releaseSize: null,
    phase,
  } : null;
  return {
    workKey: item.workKey,
    info: {
      canonicalKey: item.workKey,
      title: item.title,
      subtitle: null,
      authors: ['Frank Herbert'],
      year: 1965,
      coverId: null,
      description: 'Set on the desert planet Arrakis.',
      subjects: ['Science fiction'],
      awards: [],
      series: [],
      editionCount: 1,
      pagesMedian: 608,
      languages: ['English'],
      hasAcceptedEdition: true,
      rating: { average: 4.3, count: 10 },
    },
    infoError: null,
    status: {
      request,
      onServer: item.onServer ?? null,
      active: 0,
      cap: 5,
      otherRequests: [],
    },
  };
}

test('catalog parity shots', async ({ browser }) => {
  test.skip(!process.env.MEWO_SHOTS, 'set MEWO_SHOTS=1 to write screenshots');
  test.setTimeout(180_000);
  const requestable = work('OL1W', 'Dune', 'Frank Herbert', 1965, { coverId: null });
  const searching = work('OL2W', 'Searching Dune', 'Frank Herbert', 1965, {
    coverId: null,
    request: parityRequest('req-search', 'OL2W', 'Searching Dune'),
  });
  const downloading = work('OL3W', 'Downloading Dune', 'Frank Herbert', 1965, {
    coverId: null,
    request: parityRequest('req-dl', 'OL3W', 'Downloading Dune', { state: 'downloading', progress: 42 }),
  });
  const failed = work('OL4W', 'Failed Dune', 'Frank Herbert', 1965, {
    coverId: null,
    request: parityRequest('req-fail', 'OL4W', 'Failed Dune', {
      state: 'failed', error: 'No release found.',
    }),
  });
  const canceled = work('OL5W', 'Canceled Dune', 'Frank Herbert', 1965, {
    coverId: null,
    request: parityRequest('req-cancel', 'OL5W', 'Canceled Dune', { state: 'canceled' }),
  });
  const shelf = work('OL6W', 'Shelf Dune', 'Frank Herbert', 1965, {
    coverId: null,
    onServer: { bookId: 'book-dune', ownerUsername: 'ada', mine: true, shared: false, held: false },
  });
  const server = work('OL7W', 'Server Dune', 'Frank Herbert', 1965, {
    coverId: null,
    onServer: { bookId: 'book-bea', ownerUsername: 'bea', mine: false, shared: true, held: false },
  });
  const imported = work('OL8W', 'Imported Dune', 'Frank Herbert', 1965, {
    coverId: null,
    request: parityRequest('req-import', 'OL8W', 'Imported Dune', {
      state: 'imported', progress: 100, bookId: null,
    }),
  });
  const rows = [requestable, searching, downloading, failed, canceled, shelf, server, imported];
  const details: Record<string, unknown> = {
    OL1W: parityDetail(requestable, null),
    OL2W: parityDetail(searching, 'searching'),
    OL3W: parityDetail(downloading, 'downloading'),
    OL4W: parityDetail(failed, 'failed'),
    OL5W: parityDetail(canceled, 'canceled'),
    OL6W: parityDetail(shelf, null),
    OL7W: parityDetail(server, null),
    OL8W: parityDetail(imported, 'imported'),
  };

  for (const theme of ['x_light', 'x_lights_out'] as const) {
    const context = await browser.newContext({
      viewport: { width: 390, height: 844 },
      baseURL: 'http://127.0.0.1:4173',
    });
    const page = await context.newPage();
    const state = await install(page, { kind: 'catalog', theme });
    state.items = rows;
    state.details = details;
    state.holdCatalog = true;
    await page.goto('/search');
    await expect(page.locator('.skeletons .catalog').first()).toBeVisible();
    await expect(page.locator('.slot')).toHaveCount(0);
    await expect(page.getByRole('heading', { name: 'Trending on Open Library' })).toBeVisible();
    await shot(page, `catalog-loading-${theme}`);
    state.releaseCatalog();
    await expect(page.getByRole('button', { name: 'Request', exact: true })).toBeVisible();
    await expect(page.getByRole('button', { name: 'Request again', exact: true })).toHaveCount(2);
    await expect(page.getByRole('button', { name: 'Retry', exact: true })).toBeVisible();
    await expect(page.locator('.book-row.live')).toHaveCount(2);
    await shot(page, `catalog-rows-${theme}`);
    await page.locator('main').evaluate((el) => { el.scrollTop = el.scrollHeight; });
    await shot(page, `catalog-rows-end-${theme}`);
    await page.locator('main').evaluate((el) => { el.scrollTop = 0; });

    await page.getByPlaceholder('Search').fill('du');
    await expect(page.getByRole('heading', { name: 'Trending on Open Library' })).toBeVisible();
    await shot(page, `catalog-short-du-${theme}`);
    await page.getByPlaceholder('Search').fill('');
    await expect(page.getByText('Imported Dune')).toBeVisible();

    const openSheet = async (name: string, file: string, ready: string) => {
      await page.getByRole('button', { name, exact: true }).click();
      const sheet = page.getByRole('dialog');
      await expect(sheet.getByText(ready).first()).toBeVisible();
      await expect(sheet.getByRole('link', { name: 'View on Open Library' })).toBeVisible();
      await shot(page, `${file}-${theme}`);
      await sheet.getByRole('button', { name: 'Close' }).click();
      await expect(sheet).toHaveCount(0);
    };
    await page.getByRole('button', { name: 'Dune, Frank Herbert, 1965', exact: true }).click();
    const sheet = page.getByRole('dialog', { name: 'Dune' });
    await expect(sheet.getByRole('heading', { name: 'Dune' })).toHaveCount(1);
    await expect(sheet.getByRole('button', { name: 'Request', exact: true })).toBeVisible();
    await expect(sheet.getByRole('link', { name: 'View on Open Library' })).toBeVisible();
    await shot(page, `sheet-requestable-${theme}`);
    await sheet.getByRole('button', { name: 'Close' }).click();

    await openSheet('Searching Dune, Frank Herbert, 1965, Searching', 'sheet-searching', 'Looking for a release');
    await openSheet('Downloading Dune, Frank Herbert, 1965, Downloading 42%', 'sheet-downloading', 'Downloading 42%');
    await openSheet('Failed Dune, Frank Herbert, 1965, Failed: No release found.', 'sheet-failed', 'No release found.');
    await openSheet('Shelf Dune, Frank Herbert, 1965, On your shelf', 'sheet-shelf', 'Open book');
    await page.getByRole('button', { name: 'Imported Dune, Frank Herbert, 1965', exact: true }).click();
    const again = page.getByRole('dialog', { name: 'Imported Dune' });
    await expect(again.getByRole('button', { name: 'Request again', exact: true })).toBeVisible();
    await expect(again.getByRole('button', { name: 'Open book' })).toHaveCount(0);
    await expect(again.getByText('On your shelf')).toHaveCount(0);
    await expect(again.getByRole('link', { name: 'View on Open Library' })).toBeVisible();
    await shot(page, `sheet-imported-${theme}`);
    await again.getByRole('button', { name: 'Close' }).click();
    await openSheet('Canceled Dune, Frank Herbert, 1965', 'sheet-canceled', 'Request again');

    state.emptyBrowse = true;
    await page.reload();
    await expect(page.getByRole('heading', { name: /No trending books/ })).toBeVisible();
    await expect(page.getByRole('heading', { name: 'Trending on Open Library' })).toHaveCount(0);
    await shot(page, `catalog-empty-${theme}`);

    state.emptyBrowse = false;
    state.catalogFailure = {
      query: 'dune',
      status: 502,
      error: 'Open Library is not answering. Try again.',
    };
    await page.getByPlaceholder('Search').fill('dune');
    await expect(page.getByRole('alert')).toHaveText('Open Library is not answering.');
    await expect(page.getByRole('heading', { name: 'Trending on Open Library' })).toHaveCount(0);
    await shot(page, `catalog-error-${theme}`);

    await page.goto('/');
    await page.getByRole('tab', { name: 'Books' }).click();
    await page.getByRole('button', { name: 'Add a book', exact: true }).click();
    await expect(page.getByRole('menu')).toBeVisible();
    await shot(page, `add-menu-${theme}`);
    await context.close();
  }
});
