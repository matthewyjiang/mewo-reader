import { expect, test, type Page, type Route } from '@playwright/test';
import { mkdir } from 'node:fs/promises';

const info = {
  users: 2,
  books: 3,
  comments: 4,
  reposts: 1,
  storageBytes: 40_500_000,
};

function freshState() {
  return {
    settings: { allowSignup: true, maxEpubBytes: 64 * 1024 * 1024 },
    ada: {
      username: 'ada',
      joinedAt: 1_609_459_200_000,
      isAdmin: true,
      books: 2,
      mine: true,
    },
    byron: {
      username: 'byron',
      joinedAt: 1_609_459_300_000,
      isAdmin: false,
      books: 1,
      mine: false,
    },
  };
}

async function install(
  page: Page,
  options: { admin?: boolean; theme?: 'x_light' | 'x_lights_out'; signupError?: string } = {},
) {
  const admin = options.admin ?? true;
  const theme = options.theme ?? 'x_light';
  await page.addInitScript((nextTheme) => {
    localStorage.setItem('mewo.session', JSON.stringify({ token: 'token', username: 'ada' }));
    localStorage.setItem('mewo.display', JSON.stringify({
      theme: nextTheme, step: 2, mewo: false, keepBars: true,
    }));
  }, theme);
  const state = freshState();
  await page.route('**/v1/**', (route) => answer(route, admin, options.signupError, state));
}

async function answer(
  route: Route,
  admin: boolean,
  signupError: string | undefined,
  state: ReturnType<typeof freshState>,
) {
  const url = new URL(route.request().url());
  const path = url.pathname;
  const method = route.request().method();
  const json = (body: unknown, status = 200) => route.fulfill({
    status,
    contentType: 'application/json',
    body: JSON.stringify(body),
  });
  if (path === '/v1/me') return json({ username: 'ada', isAdmin: admin });
  if (path === '/v1/library') return json({ books: [] });
  if (path === '/v1/feed') return json({ items: [], nextCursor: null });
  if (!admin && path.startsWith('/v1/admin/')) {
    return json({ error: 'Admins only.' }, 403);
  }
  if (path === '/v1/admin/settings' && method === 'PATCH') {
    if (signupError) return json({ error: signupError }, 400);
    const patch = route.request().postDataJSON() as Partial<typeof state.settings>;
    Object.assign(state.settings, patch);
    return json(state.settings);
  }
  if (path === '/v1/admin/settings') return json(state.settings);
  if (path === '/v1/admin/server') return json(info);
  if (path === '/v1/admin/users' && method === 'GET') return json({ items: [state.ada, state.byron] });
  if (path.startsWith('/v1/admin/users/') && method === 'PATCH') {
    const username = decodeURIComponent(path.slice('/v1/admin/users/'.length));
    const row = username === 'byron' ? state.byron : state.ada;
    const patch = route.request().postDataJSON() as { isAdmin: boolean };
    row.isAdmin = patch.isAdmin;
    return json(row);
  }
  return json({});
}

test('an admin can open Server and the signups toggle patches', async ({ page }) => {
  await page.setViewportSize({ width: 390, height: 844 });
  await install(page);
  await page.goto('/settings');
  await page.getByRole('button', { name: 'Server' }).click();
  await expect(page.getByRole('switch', { name: 'Signups' })).toBeChecked();
  await expect(page.getByText('38.6 MB')).toBeVisible();
  const patch = page.waitForRequest((request) =>
    request.method() === 'PATCH' && request.url().includes('/v1/admin/settings'));
  await page.getByRole('switch', { name: 'Signups' }).click();
  expect((await patch).postDataJSON()).toEqual({ allowSignup: false });
  await expect(page.getByRole('switch', { name: 'Signups' })).not.toBeChecked();
});

test('a settings 400 shows the server message', async ({ page }) => {
  await page.setViewportSize({ width: 390, height: 844 });
  const error = 'maxEpubBytes must be 1 to 1024 MB.';
  await install(page, { signupError: error });
  await page.goto('/settings/server');
  await page.getByRole('switch', { name: 'Signups' }).click();
  await expect(page.getByRole('status')).toContainText(error);
  await expect(page.getByRole('switch', { name: 'Signups' })).toBeChecked();
});

test('your own admin toggle is disabled', async ({ page }) => {
  await page.setViewportSize({ width: 390, height: 844 });
  await install(page);
  await page.goto('/settings/server/users');
  const mine = page.getByRole('button', { name: 'ada admin' });
  await expect(mine).toHaveAttribute('aria-disabled', 'true');
  await expect(mine).not.toHaveAttribute('disabled');
  await expect(mine).toHaveAttribute('aria-pressed', 'true');
  const other = page.getByRole('button', { name: 'byron admin' });
  await expect(other).toBeEnabled();
  await expect(other).toHaveAttribute('aria-pressed', 'false');
  await page.getByRole('button', { name: '@byron' }).click();
  await expect(page).toHaveURL(/\/profiles\/byron$/);
});

test('an out-of-range EPUB size is rejected before the request', async ({ page }) => {
  await page.setViewportSize({ width: 390, height: 844 });
  await install(page);
  await page.goto('/settings/server');
  const input = page.locator('.epub-field input');
  await expect(input).toHaveValue('64');
  let patched = false;
  page.on('request', (request) => {
    if (request.method() === 'PATCH') patched = true;
  });
  await input.fill('2000');
  await input.press('Enter');
  await expect(page.getByRole('status')).toContainText('maxEpubBytes must be 1 to 1024 MB.');
  await expect(input).toHaveValue('64');
  expect(patched).toBe(false);
});

test('/v1/me loads once per token', async ({ page }) => {
  await page.setViewportSize({ width: 390, height: 844 });
  await install(page);
  let me = 0;
  page.on('request', (request) => {
    if (new URL(request.url()).pathname === '/v1/me') me += 1;
  });
  await page.goto('/settings');
  await expect(page.getByRole('button', { name: 'Server' })).toBeVisible();
  await page.getByRole('button', { name: 'About' }).click();
  await expect(page.getByRole('heading', { name: 'About' })).toBeVisible();
  await page.goBack();
  await expect(page.getByRole('button', { name: 'Server' })).toBeVisible();
  expect(me).toBe(1);
});

test('a non-admin has no Server row and the panel is not found', async ({ page }) => {
  await page.setViewportSize({ width: 390, height: 844 });
  await install(page, { admin: false });
  await page.goto('/settings');
  await expect(page.getByRole('button', { name: 'About' })).toBeVisible();
  await expect(page.getByRole('button', { name: 'Server' })).toHaveCount(0);
  await page.goto('/settings/server');
  await expect(page.getByRole('heading', { name: 'Not found' })).toBeVisible();
});

async function signInPage(page: Page, allowSignup: boolean | 'fail') {
  await page.addInitScript(() => {
    localStorage.setItem('mewo.display', JSON.stringify({
      theme: 'x_light', step: 2, mewo: false, keepBars: true,
    }));
  });
  await page.route('**/v1/instance', async (route) => {
    if (allowSignup === 'fail') {
      await route.fulfill({ status: 500, contentType: 'application/json', body: '{}' });
      return;
    }
    await route.fulfill({
      status: 200,
      contentType: 'application/json',
      body: JSON.stringify({ allowSignup }),
    });
  });
}

test('sign-in keeps Create an account until signups are known to be off', async ({ page }) => {
  await page.setViewportSize({ width: 390, height: 844 });
  let release = () => {};
  const gate = new Promise<void>((resolve) => {
    release = resolve;
  });
  await page.addInitScript(() => {
    localStorage.setItem('mewo.display', JSON.stringify({
      theme: 'x_light', step: 2, mewo: false, keepBars: true,
    }));
  });
  await page.route('**/v1/instance', async (route) => {
    await gate;
    await route.fulfill({
      status: 200,
      contentType: 'application/json',
      body: JSON.stringify({ allowSignup: false }),
    });
  });
  await page.goto('/');
  await expect(page.getByRole('button', { name: 'Create an account' })).toBeVisible();
  release();
  await expect(page.getByRole('button', { name: 'Create an account' })).toHaveCount(0);
  await expect(page.getByRole('heading', { name: 'Sign in' })).toBeVisible();
});

test('sign-in shows Create an account when the instance call fails', async ({ page }) => {
  await page.setViewportSize({ width: 390, height: 844 });
  await signInPage(page, 'fail');
  await page.goto('/');
  await expect(page.getByRole('button', { name: 'Create an account' })).toBeVisible();
});

test('shots of the admin panel and sign-in', async ({ browser }) => {
  test.skip(!process.env.MEWO_SHOTS, 'set MEWO_SHOTS=1 to write admin screenshots');
  const dir = process.env.MEWO_SHOTS_DIR ?? 'test-results/admin-shots';
  await mkdir(dir, { recursive: true });
  const shots: { theme: 'x_light' | 'x_lights_out'; width: number }[] = [];
  for (const theme of ['x_light', 'x_lights_out'] as const) {
    for (const width of [1280, 390]) shots.push({ theme, width });
  }
  for (const { theme, width } of shots) {
    const label = `${width}-${theme === 'x_light' ? 'light' : 'dark'}`;
    const context = await browser.newContext({
      viewport: { width, height: width === 390 ? 844 : 900 },
      colorScheme: theme === 'x_light' ? 'light' : 'dark',
    });
    const page = await context.newPage();
    try {
      await install(page, { theme });
      await page.goto('/settings');
      await expect(page.getByRole('button', { name: 'Server' })).toBeVisible();
      await page.screenshot({ path: `${dir}/web-settings-${label}.png` });
      await page.getByRole('button', { name: 'Server' }).click();
      await expect(page.getByText('38.6 MB')).toBeVisible();
      await page.screenshot({ path: `${dir}/web-settings-server-${label}.png` });
      await page.getByRole('button', { name: 'Users' }).click();
      await expect(page.getByRole('button', { name: 'ada admin' })).toBeVisible();
      await page.screenshot({ path: `${dir}/web-settings-server-users-${label}.png` });
    } finally {
      await context.close();
    }

    for (const open of [true, false]) {
      const auth = await browser.newContext({
        viewport: { width, height: width === 390 ? 844 : 900 },
        colorScheme: theme === 'x_light' ? 'light' : 'dark',
      });
      const sign = await auth.newPage();
      try {
        await sign.addInitScript((nextTheme) => {
          localStorage.setItem('mewo.display', JSON.stringify({
            theme: nextTheme, step: 2, mewo: false, keepBars: true,
          }));
        }, theme);
        await sign.route('**/v1/instance', (route) => route.fulfill({
          status: 200,
          contentType: 'application/json',
          body: JSON.stringify({ allowSignup: open }),
        }));
        await sign.goto('/');
        const create = sign.getByRole('button', { name: 'Create an account' });
        if (open) await expect(create).toBeVisible();
        else await expect(create).toHaveCount(0);
        await expect(sign.getByRole('heading', { name: 'Sign in' })).toBeVisible();
        const state = open ? 'on' : 'off';
        await sign.screenshot({ path: `${dir}/web-sign-in-${state}-${label}.png` });
      } finally {
        await auth.close();
      }
    }
  }
});
