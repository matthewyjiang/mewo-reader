import { expect, test, type Page, type Route } from '@playwright/test';
import { mkdir } from 'node:fs/promises';

const DAY = 86_400_000;
const expires = Date.UTC(2027, 10, 12, 12, 0, 0);

interface Invite {
  id: string;
  note: string | null;
  createdBy: string | null;
  createdAt: number;
  expiresAt: number;
  usedAt: number | null;
  usedBy: string | null;
  revokedAt: number | null;
  state: 'active' | 'used' | 'expired' | 'revoked';
}

function row(overrides: Partial<Invite> & Pick<Invite, 'id' | 'state'>): Invite {
  return {
    note: null,
    createdBy: 'ada',
    createdAt: expires - 7 * DAY,
    expiresAt: expires,
    usedAt: null,
    usedBy: null,
    revokedAt: null,
    ...overrides,
  };
}

function themeScript(theme: 'x_light' | 'x_lights_out'): void {
  localStorage.setItem('mewo.display', JSON.stringify({
    theme, step: 2, mewo: false, keepBars: true,
  }));
}

async function signedOut(
  page: Page,
  options: { allowSignup?: boolean; check?: 'valid' | 'used' | 'expired' | 'invalid' | 'fail' | 'hold' } = {},
) {
  const allowSignup = options.allowSignup ?? false;
  const check = options.check ?? 'valid';
  await page.addInitScript(themeScript, 'x_light');
  let release = () => {};
  const held = new Promise<void>((resolve) => {
    release = resolve;
  });
  const seen: string[] = [];
  await page.route('**/v1/**', async (route) => {
    const url = new URL(route.request().url());
    const path = url.pathname;
    const method = route.request().method();
    const json = (body: unknown, status = 200) => route.fulfill({
      status,
      contentType: 'application/json',
      body: JSON.stringify(body),
    });
    if (path === '/v1/instance') return json({ allowSignup });
    if (path === '/v1/invites/check' && method === 'POST') {
      const token = (route.request().postDataJSON() as { token: string }).token;
      seen.push(token);
      if (check === 'hold' && token === 'old') await held;
      if (check === 'fail') return json({ error: 'nope' }, 500);
      const status = check === 'hold' ? (token === 'new' ? 'valid' : 'invalid') : check;
      return json({ status });
    }
    if (path === '/v1/auth/register' && method === 'POST') {
      const body = route.request().postDataJSON() as { username: string };
      return json({ token: 'session', username: body.username });
    }
    if (path === '/v1/auth/login' && method === 'POST') {
      const body = route.request().postDataJSON() as { username: string };
      return json({ token: 'session', username: body.username });
    }
    if (path === '/v1/me') return json({ username: 'ada', isAdmin: false, canRequest: false });
    if (path === '/v1/library') return json({ books: [] });
    if (path === '/v1/feed') return json({ items: [], nextCursor: null });
    return json({});
  });
  return { seen, release };
}

async function fillAccount(page: Page): Promise<void> {
  await page.getByLabel('Username').fill('ada');
  await page.getByLabel('Password').fill('password1');
}

test('a valid invite registers while signups are off and leaves the query behind', async ({ page }) => {
  await page.setViewportSize({ width: 390, height: 844 });
  await signedOut(page, { allowSignup: false, check: 'valid' });
  await page.goto('/signup?invite=abc');
  await expect(page.getByRole('heading', { name: 'Create account' })).toBeVisible();
  await expect(page.getByLabel('Username')).toBeEnabled();
  const posted = page.waitForRequest((request) =>
    request.method() === 'POST' && request.url().includes('/v1/auth/register'));
  await fillAccount(page);
  await page.getByRole('button', { name: 'Create account' }).click();
  expect((await posted).postDataJSON()).toEqual({
    username: 'ada', password: 'password1', invite: 'abc',
  });
  await expect(page).toHaveURL(/\/$/);
  await expect(page).not.toHaveURL(/invite=/);
});

test('signups off without an invite still hides Create an account', async ({ page }) => {
  await page.setViewportSize({ width: 390, height: 844 });
  await signedOut(page, { allowSignup: false });
  await page.goto('/');
  await expect(page.getByRole('heading', { name: 'Sign in' })).toBeVisible();
  await expect(page.getByRole('button', { name: 'Create an account' })).toHaveCount(0);
});

test('ordinary sign-in keeps the page it started on', async ({ page }) => {
  await page.setViewportSize({ width: 390, height: 844 });
  await signedOut(page, { allowSignup: true });
  await page.goto('/search');
  await fillAccount(page);
  await page.getByRole('button', { name: 'Sign in', exact: true }).click();
  await expect(page).toHaveURL(/\/search$/);
});

test('Sign in instead does not send the invite', async ({ page }) => {
  await page.setViewportSize({ width: 390, height: 844 });
  await signedOut(page, { allowSignup: true, check: 'valid' });
  await page.goto('/signup?invite=abc');
  await expect(page.getByRole('heading', { name: 'Create account' })).toBeVisible();
  await page.getByRole('button', { name: 'Sign in instead' }).click();
  await expect(page.getByRole('heading', { name: 'Sign in' })).toBeVisible();
  const posted = page.waitForRequest((request) =>
    request.method() === 'POST' && request.url().includes('/v1/auth/login'));
  await fillAccount(page);
  await page.getByRole('button', { name: 'Sign in', exact: true }).click();
  expect((await posted).postDataJSON()).toEqual({ username: 'ada', password: 'password1' });
});

for (const status of ['used', 'expired', 'invalid'] as const) {
  for (const open of [true, false]) {
    test(`a ${status} invite dismisses to login when signups are ${open ? 'open' : 'closed'}`, async ({ page }) => {
      await page.setViewportSize({ width: 390, height: 844 });
      await signedOut(page, { allowSignup: open, check: status });
      await page.goto('/signup?invite=abc');
      const heading = status === 'used'
        ? 'Invite already used'
        : status === 'expired' ? 'Invite expired' : 'Invite not valid';
      await expect(page.getByRole('heading', { name: heading })).toBeVisible();
      await expect(page.getByLabel('Username')).toHaveCount(0);
      await page.getByRole('button', { name: 'Sign in', exact: true }).click();
      await expect(page).toHaveURL(/\/$/);
      await expect(page).not.toHaveURL(/invite=/);
      await expect(page.getByRole('heading', { name: 'Sign in' })).toBeVisible();
      const posted = page.waitForRequest((request) =>
        request.method() === 'POST' && request.url().includes('/v1/auth/login'));
      await fillAccount(page);
      await page.getByRole('button', { name: 'Sign in', exact: true }).click();
      const request = await posted;
      expect(new URL(request.url()).pathname).toBe('/v1/auth/login');
      expect(request.postDataJSON()).toEqual({ username: 'ada', password: 'password1' });
    });
  }
}

test('a failed invite check keeps the form and lets submit decide', async ({ page }) => {
  await page.setViewportSize({ width: 390, height: 844 });
  await signedOut(page, { allowSignup: false, check: 'fail' });
  await page.goto('/signup?invite=abc');
  await expect(page.getByRole('heading', { name: 'Create account' })).toBeVisible();
  await expect(page.getByLabel('Username')).toBeEnabled();
  const posted = page.waitForRequest((request) =>
    request.method() === 'POST' && request.url().includes('/v1/auth/register'));
  await fillAccount(page);
  await page.getByRole('button', { name: 'Create account' }).click();
  expect((await posted).postDataJSON()).toMatchObject({ invite: 'abc' });
});

test('a stale invite check cannot replace the current token', async ({ page }) => {
  await page.setViewportSize({ width: 390, height: 844 });
  const { release } = await signedOut(page, { check: 'hold' });
  await page.goto('/signup?invite=old');
  await expect(page.getByLabel('Username')).toBeDisabled();
  await page.evaluate(() => {
    history.pushState(history.state, '', '/signup?invite=new');
    window.dispatchEvent(new PopStateEvent('popstate'));
  });
  await expect(page.getByLabel('Username')).toBeEnabled();
  await expect(page.getByRole('heading', { name: 'Create account' })).toBeVisible();
  release();
  await page.waitForTimeout(300);
  await expect(page.getByRole('heading', { name: 'Invite not valid' })).toHaveCount(0);
  await expect(page.getByLabel('Username')).toBeEnabled();
});

test('a signed-in visit to an invite link goes home without checking it', async ({ page }) => {
  await page.setViewportSize({ width: 390, height: 844 });
  await page.addInitScript(() => {
    localStorage.setItem('mewo.session', JSON.stringify({ token: 'token', username: 'ada' }));
    localStorage.setItem('mewo.display', JSON.stringify({
      theme: 'x_light', step: 2, mewo: false, keepBars: true,
    }));
  });
  let checks = 0;
  await page.route('**/v1/**', async (route) => {
    const path = new URL(route.request().url()).pathname;
    if (path === '/v1/invites/check') checks += 1;
    const body = path === '/v1/me'
      ? { username: 'ada', isAdmin: false, canRequest: false }
      : path === '/v1/library' ? { books: [] }
        : path === '/v1/feed' ? { items: [], nextCursor: null }
          : {};
    await route.fulfill({
      status: 200,
      contentType: 'application/json',
      body: JSON.stringify(body),
    });
  });
  await page.goto('/signup?invite=abc');
  await expect(page).toHaveURL(/\/$/);
  await page.waitForTimeout(200);
  expect(checks).toBe(0);
});

test('a stale session keeps the invite and shows the signup form', async ({ page }) => {
  await page.setViewportSize({ width: 390, height: 844 });
  await page.addInitScript(() => {
    localStorage.setItem('mewo.session', JSON.stringify({ token: 'stale', username: 'ada' }));
    localStorage.setItem('mewo.display', JSON.stringify({
      theme: 'x_light', step: 2, mewo: false, keepBars: true,
    }));
  });
  await page.route('**/v1/**', async (route) => {
    const path = new URL(route.request().url()).pathname;
    const method = route.request().method();
    const json = (body: unknown, status = 200) => route.fulfill({
      status,
      contentType: 'application/json',
      body: JSON.stringify(body),
    });
    if (path === '/v1/me') return json({ error: 'Sign in first.' }, 401);
    if (path === '/v1/instance') return json({ allowSignup: false });
    if (path === '/v1/invites/check' && method === 'POST') return json({ status: 'valid' });
    return json({ error: 'Sign in first.' }, 401);
  });
  await page.goto('/signup?invite=abc');
  await expect(page).toHaveURL(/\/signup\?invite=abc$/);
  await expect(page.getByRole('heading', { name: 'Create account' })).toBeVisible();
  await expect(page.getByLabel('Username')).toBeVisible();
});

function installAdmin(
  page: Page,
  options: { admin?: boolean; theme?: 'x_light' | 'x_lights_out'; clipboard?: 'ok' | 'fail' } = {},
) {
  const admin = options.admin ?? true;
  const theme = options.theme ?? 'x_light';
  const invites = [
    row({ id: 'active', state: 'active', note: 'for ada' }),
    row({ id: 'used', state: 'used', note: null, usedAt: expires, usedBy: 'bea' }),
    row({ id: 'expired', state: 'expired', note: 'old' }),
    row({ id: 'revoked', state: 'revoked', note: 'nope', revokedAt: expires }),
  ];
  return page.addInitScript((next) => {
    localStorage.setItem('mewo.session', JSON.stringify({ token: 'token', username: 'ada' }));
    localStorage.setItem('mewo.display', JSON.stringify({
      theme: next.theme, step: 2, mewo: false, keepBars: true,
    }));
    if (next.clipboard === 'fail') {
      Object.defineProperty(navigator, 'clipboard', {
        configurable: true,
        value: { writeText: () => Promise.reject(new Error('denied')) },
      });
    }
  }, { theme, clipboard: options.clipboard ?? 'ok' }).then(() => page.route('**/v1/**', (route) => answer(route, admin, invites)));
}

async function answer(route: Route, admin: boolean, invites: Invite[]) {
  const url = new URL(route.request().url());
  const path = url.pathname;
  const method = route.request().method();
  const json = (body: unknown, status = 200) => route.fulfill({
    status,
    contentType: 'application/json',
    body: JSON.stringify(body),
  });
  if (path === '/v1/me') return json({ username: 'ada', isAdmin: admin, canRequest: false });
  if (path === '/v1/library') return json({ books: [] });
  if (path === '/v1/feed') return json({ items: [], nextCursor: null });
  if (!admin && path.startsWith('/v1/admin/')) return json({ error: 'Admins only.' }, 403);
  if (path === '/v1/admin/server') {
    return json({
      users: 2, books: 0, comments: 0, reposts: 0, storageBytes: 0,
      invites: invites.filter((invite) => invite.state === 'active').length,
    });
  }
  if (path === '/v1/admin/settings') {
    return json({
      allowSignup: false,
      maxEpubBytes: 64 * 1024 * 1024,
      requestsEnabled: false,
      prowlarrUrl: '',
      sabnzbdUrl: '',
      sabnzbdCategory: 'books',
      downloadsDir: null,
      languages: ['en'],
      supportedLanguages: [{ code: 'en', name: 'English' }],
      secrets: { prowlarrApiKey: false, sabnzbdApiKey: false },
    });
  }
  if (path === '/v1/admin/invites' && method === 'GET') return json({ items: invites });
  if (path === '/v1/admin/invites' && method === 'POST') {
    const body = route.request().postDataJSON() as { note?: string; days: number };
    const created = row({
      id: `made-${invites.length}`,
      state: 'active',
      note: body.note ?? null,
      expiresAt: Date.now() + body.days * DAY,
    });
    invites.unshift(created);
    return json({ invite: created, token: 'ab'.repeat(32) });
  }
  if (path.endsWith('/revoke') && method === 'POST') {
    const id = decodeURIComponent(path.slice('/v1/admin/invites/'.length, -'/revoke'.length));
    const invite = invites.find((item) => item.id === id);
    if (!invite) return json({ error: 'Not found.' }, 404);
    if (invite.state === 'used') return json({ error: 'This invite was already used.' }, 409);
    invite.state = 'revoked';
    invite.revokedAt = Date.now();
    return json(invite);
  }
  return json({});
}

test('the server row counts active invites and the page creates, copies once, and revokes', async ({ page }) => {
  await page.setViewportSize({ width: 390, height: 844 });
  await installAdmin(page);
  await page.goto('/settings/server');
  const rowButton = page.getByRole('button', { name: 'Invites' });
  await expect(rowButton).toContainText('1');
  await rowButton.click();
  await expect(page.getByRole('heading', { name: 'Invites' })).toBeVisible();
  await expect(page.getByText('for ada')).toBeVisible();
  await expect(page.getByText('@bea')).toBeVisible();
  await expect(page.getByText('Expires Nov 12')).toBeVisible();
  await page.getByLabel('Note').fill('  for bea  ');
  const created = page.waitForRequest((request) =>
    request.method() === 'POST' && request.url().endsWith('/v1/admin/invites'));
  await page.getByRole('button', { name: 'Create invite' }).click();
  expect((await created).postDataJSON()).toEqual({ note: 'for bea', days: 7 });
  const link = page.getByLabel('Invite link');
  await expect(link).toHaveValue(/\/signup\?invite=abab/);
  await expect(link).toHaveAttribute('readonly', '');
  await page.goto('/settings/server');
  await page.getByRole('button', { name: 'Invites' }).click();
  await expect(page.getByLabel('Invite link')).toHaveCount(0);
  const active = page.locator('.settings-row', { hasText: 'for ada' });
  await active.getByRole('button', { name: 'Revoke' }).click();
  await expect(active).toContainText('Revoked');
  await expect(active.getByRole('button', { name: 'Revoke' })).toHaveCount(0);
});

test('the expiry field accepts 14 days', async ({ page }) => {
  await page.setViewportSize({ width: 390, height: 844 });
  await installAdmin(page);
  await page.goto('/settings/server/invites');
  await page.getByLabel('Expires in days').fill('14');
  const created = page.waitForRequest((request) =>
    request.method() === 'POST' && request.url().endsWith('/v1/admin/invites'));
  await page.getByRole('button', { name: 'Create invite' }).click();
  expect((await created).postDataJSON()).toEqual({ days: 14 });
});

test('revoke of a used invite refreshes the row and shows the message', async ({ page }) => {
  await page.setViewportSize({ width: 390, height: 844 });
  await page.addInitScript(themeScript, 'x_light');
  await page.addInitScript(() => {
    localStorage.setItem('mewo.session', JSON.stringify({ token: 'token', username: 'ada' }));
  });
  const invites = [row({ id: 'active', state: 'active', note: 'for ada' })];
  await page.route('**/v1/**', async (route) => {
    const path = new URL(route.request().url()).pathname;
    if (path.endsWith('/revoke')) {
      invites[0] = { ...invites[0], state: 'used', usedAt: Date.now(), usedBy: 'bea' };
      return route.fulfill({
        status: 409,
        contentType: 'application/json',
        body: JSON.stringify({ error: 'This invite was already used.' }),
      });
    }
    return answer(route, true, invites);
  });
  await page.goto('/settings/server/invites');
  await page.getByRole('button', { name: 'Revoke' }).click();
  await expect(page.getByRole('status')).toContainText('This invite was already used.');
  await expect(page.getByText('@bea')).toBeVisible();
  await expect(page.getByRole('button', { name: 'Revoke' })).toHaveCount(0);
});

test('keyboard reaches create, and a failed copy leaves the link selectable', async ({ page }) => {
  await page.setViewportSize({ width: 390, height: 844 });
  await installAdmin(page, { theme: 'x_lights_out', clipboard: 'fail' });
  await page.goto('/settings/server/invites');
  await page.getByLabel('Note').focus();
  await page.keyboard.type('ada');
  await page.keyboard.press('Tab');
  await expect(page.getByLabel('Expires in days')).toBeFocused();
  await page.keyboard.press('Tab');
  await expect(page.getByRole('button', { name: 'Create invite' })).toBeFocused();
  await page.keyboard.press('Enter');
  const link = page.getByLabel('Invite link');
  await expect(link).toBeVisible();
  await page.getByRole('button', { name: 'Copy link' }).click();
  await expect(page.getByRole('button', { name: 'Copied' })).toHaveCount(0);
  await expect(link).toHaveAttribute('readonly', '');
  await expect(link).toHaveValue(/signup\?invite=/);
});

test('a non-admin sees Not found', async ({ page }) => {
  await page.setViewportSize({ width: 390, height: 844 });
  await installAdmin(page, { admin: false });
  await page.goto('/settings/server/invites');
  await expect(page.getByRole('heading', { name: 'Not found' })).toBeVisible();
  await expect(page.getByRole('button', { name: 'Back to Settings' })).toBeVisible();
});

test('shots of invites and signup states', async ({ browser }) => {
  test.skip(!process.env.MEWO_SHOTS, 'set MEWO_SHOTS=1 to write invite screenshots');
  const dir = process.env.MEWO_SHOTS_DIR ?? 'test-results/invite-shots';
  await mkdir(dir, { recursive: true });
  for (const theme of ['x_light', 'x_lights_out'] as const) {
    const label = theme === 'x_light' ? 'light' : 'dark';
    const context = await browser.newContext({
      viewport: { width: 390, height: 844 },
      colorScheme: theme === 'x_light' ? 'light' : 'dark',
    });
    const page = await context.newPage();
    try {
      await installAdmin(page, { theme });
      await page.goto('/settings/server/invites');
      await expect(page.getByRole('button', { name: 'Create invite' })).toBeVisible();
      await page.getByLabel('Note').fill('saturday');
      await page.getByRole('button', { name: 'Create invite' }).click();
      await expect(page.getByLabel('Invite link')).toBeVisible();
      await page.screenshot({ path: `${dir}/invites-${label}-narrow.png` });
    } finally {
      await context.close();
    }

    for (const status of ['valid', 'used', 'expired', 'invalid'] as const) {
      const auth = await browser.newContext({
        viewport: { width: 390, height: 844 },
        colorScheme: theme === 'x_light' ? 'light' : 'dark',
      });
      const sign = await auth.newPage();
      try {
        await signedOut(sign, { allowSignup: false, check: status });
        await sign.addInitScript(themeScript, theme);
        await sign.goto('/signup?invite=abc');
        const heading = status === 'valid' ? 'Create account'
          : status === 'used' ? 'Invite already used'
            : status === 'expired' ? 'Invite expired' : 'Invite not valid';
        await expect(sign.getByRole('heading', { name: heading })).toBeVisible();
        await sign.screenshot({ path: `${dir}/signup-${status}-${label}-narrow.png` });
      } finally {
        await auth.close();
      }
    }
  }
});
