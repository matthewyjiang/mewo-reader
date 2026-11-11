import { expect, test, type Page, type Route } from '@playwright/test';
import { mkdir } from 'node:fs/promises';

const info = {
  users: 2,
  books: 3,
  comments: 4,
  reposts: 1,
  storageBytes: 40_500_000,
  invites: 1,
};

function freshState() {
  return {
    meFails: 0,
    settings: {
      allowSignup: true,
      maxEpubBytes: 64 * 1024 * 1024,
      languages: ['en'],
      supportedLanguages: [
        { code: 'en', name: 'English' },
        { code: 'de', name: 'German' },
      ],
    },
    ada: {
      username: 'ada',
      joinedAt: 1_609_459_200_000,
      isAdmin: true,
      books: 2,
      mine: true,
      disabled: false,
    },
    byron: {
      username: 'byron',
      joinedAt: 1_609_459_300_000,
      isAdmin: false,
      books: 1,
      mine: false,
      disabled: false,
    },
    deleted: false,
    deleteError: '',
    resets: 0,
  };
}

const removal = {
  booksDeleted: 3,
  sharedDeleted: 2,
  booksPassed: 1,
  holdsReleased: 0,
  requestsCanceled: 0,
};

async function install(
  page: Page,
  options: {
    admin?: boolean;
    theme?: 'x_light' | 'x_lights_out';
    signupError?: string;
    meFails?: number;
    maxEpubBytes?: number;
  } = {},
) {
  const admin = options.admin ?? true;
  const theme = options.theme ?? 'x_light';
  await page.addInitScript((nextTheme) => {
    localStorage.setItem('mewo.session', JSON.stringify({ token: 'token', username: 'ada' }));
    localStorage.setItem('mewo.display', JSON.stringify({
      theme: nextTheme, step: 2, mewo: false, keepBars: true,
    }));
    const copied: string[] = [];
    Object.assign(window, { copied });
    Object.defineProperty(navigator, 'clipboard', {
      configurable: true,
      value: { writeText: (text: string) => { copied.push(text); return Promise.resolve(); } },
    });
  }, theme);
  const state = freshState();
  state.meFails = options.meFails ?? 0;
  if (options.maxEpubBytes !== undefined) state.settings.maxEpubBytes = options.maxEpubBytes;
  await page.route('**/v1/**', (route) => answer(route, admin, options.signupError, state));
  return state;
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
  if (path === '/v1/me') {
    if (state.meFails > 0) {
      state.meFails -= 1;
      return json({ error: 'unavailable' }, 500);
    }
    return json({ username: 'ada', isAdmin: admin });
  }
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
  if (path === '/v1/admin/invites' && method === 'GET') return json({ items: [] });
  if (path === '/v1/auth/password' && method === 'POST') {
    const body = route.request().postDataJSON() as { currentPassword: string };
    if (body.currentPassword !== 'password1') return json({ error: 'Wrong password.' }, 403);
    return route.fulfill({ status: 204 });
  }
  if (path === '/v1/admin/users' && method === 'GET') {
    return json({ items: state.deleted ? [state.ada] : [state.ada, state.byron] });
  }
  if (path === '/v1/admin/users/byron/password' && method === 'POST') {
    state.resets += 1;
    return json({ password: 'abcd-efgh-jkmn-pqrs' });
  }
  if (path === '/v1/admin/users/byron/removal') return json(removal);
  if (path === '/v1/admin/users/byron' && method === 'DELETE') {
    if (state.deleteError) return json({ error: state.deleteError }, 409);
    state.deleted = true;
    return json(removal);
  }
  if (path.startsWith('/v1/admin/users/') && method === 'PATCH') {
    const username = decodeURIComponent(path.slice('/v1/admin/users/'.length));
    const row = username === 'byron' ? state.byron : state.ada;
    const patch = route.request().postDataJSON() as { isAdmin?: boolean; disabled?: boolean };
    if (patch.isAdmin !== undefined) row.isAdmin = patch.isAdmin;
    if (patch.disabled !== undefined) row.disabled = patch.disabled;
    return json(row);
  }
  return json({});
}

test('book languages open, save German, and keep the last language', async ({ page }) => {
  await page.setViewportSize({ width: 390, height: 844 });
  await install(page);
  await page.goto('/settings/server');
  await expect(page.getByText('Requests only download books in these languages.')).toBeVisible();
  const field = page.getByRole('button', { name: 'Book languages' });
  await expect(field).toHaveAttribute('aria-expanded', 'false');
  await field.click();
  const list = page.getByRole('listbox', { name: 'Book languages' });
  await expect(list).toHaveAttribute('aria-multiselectable', 'true');
  const english = page.getByRole('option', { name: 'English' });
  const german = page.getByRole('option', { name: 'German' });
  await expect(english).toHaveAttribute('aria-selected', 'true');
  await expect(german).toHaveAttribute('aria-selected', 'false');
  const add = page.waitForRequest((request) =>
    request.method() === 'PATCH' && request.url().includes('/v1/admin/settings'));
  await german.click();
  expect((await add).postDataJSON()).toEqual({ languages: ['en', 'de'] });
  await expect(list).toBeVisible();
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
  let locked = false;
  page.on('request', (request) => {
    if (request.method() === 'PATCH') locked = true;
  });
  await english.click();
  await expect(english).toHaveAttribute('aria-selected', 'true');
  await expect(field).toContainText('English');
  expect(locked).toBe(false);
});

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

test('your own row has no Manage button', async ({ page }) => {
  await page.setViewportSize({ width: 390, height: 844 });
  await install(page);
  await page.goto('/settings/server/users');
  await expect(page.getByRole('button', { name: 'Manage byron' })).toBeVisible();
  await expect(page.getByRole('button', { name: 'Manage ada' })).toHaveCount(0);
  await expect(page.getByRole('button', { name: 'ada admin' })).toBeVisible();
});

test('a reset shows the new password once and copies it', async ({ page }) => {
  await page.setViewportSize({ width: 390, height: 844 });
  const state = await install(page);
  await page.goto('/settings/server/users');
  await page.getByRole('button', { name: 'Manage byron' }).click();
  const sheet = page.getByRole('dialog', { name: 'Manage byron' });
  await sheet.getByRole('button', { name: 'Reset password' }).click();
  await expect(sheet.getByRole('heading', { name: "Reset byron's password?" })).toBeVisible();
  await expect(sheet.getByText('Signs byron out everywhere.')).toBeVisible();
  expect(state.resets).toBe(0);
  await sheet.getByRole('button', { name: 'Reset', exact: true }).click();
  const field = sheet.getByRole('textbox', { name: 'Temporary password' });
  await expect(field).toHaveValue('abcd-efgh-jkmn-pqrs');
  await expect(field).toHaveAttribute('readonly', '');
  await sheet.getByRole('button', { name: 'Copy' }).click();
  await expect(sheet.getByRole('button', { name: 'Copied' })).toBeVisible();
  expect(await page.evaluate(() => (window as unknown as { copied: string[] }).copied))
    .toEqual(['abcd-efgh-jkmn-pqrs']);
  await sheet.getByRole('button', { name: 'Done' }).click();
  await expect(sheet).toHaveCount(0);
  await page.getByRole('button', { name: 'Manage byron' }).click();
  await expect(page.getByRole('button', { name: 'Reset password' })).toBeVisible();
  await expect(page.getByText('abcd-efgh-jkmn-pqrs')).toHaveCount(0);
  await expect(page.getByRole('textbox', { name: 'Temporary password' })).toHaveCount(0);
  expect(state.resets).toBe(1);
});

test('disable asks first, tags the row, and enable runs right away', async ({ page }) => {
  await page.setViewportSize({ width: 390, height: 844 });
  await install(page);
  await page.goto('/settings/server/users');
  await expect(page.getByText('Disabled')).toHaveCount(0);
  await page.getByRole('button', { name: 'Manage byron' }).click();
  await page.getByRole('button', { name: 'Disable' }).click();
  await expect(page.getByRole('heading', { name: 'Disable byron?' })).toBeVisible();
  await expect(page.getByText('Signs byron out everywhere. Books and posts stay.')).toBeVisible();
  const disable = page.waitForRequest((request) =>
    request.method() === 'PATCH' && request.url().endsWith('/v1/admin/users/byron'));
  await page.getByRole('dialog').getByRole('button', { name: 'Disable' }).click();
  expect((await disable).postDataJSON()).toEqual({ disabled: true });
  await expect(page.getByRole('dialog')).toHaveCount(0);
  await expect(page.getByText('Disabled', { exact: true })).toBeVisible();
  await page.getByRole('button', { name: 'Manage byron' }).click();
  const enable = page.waitForRequest((request) =>
    request.method() === 'PATCH' && request.url().endsWith('/v1/admin/users/byron'));
  await page.getByRole('button', { name: 'Enable' }).click();
  expect((await enable).postDataJSON()).toEqual({ disabled: false });
  await expect(page.getByRole('dialog')).toHaveCount(0);
  await expect(page.getByText('Disabled', { exact: true })).toHaveCount(0);
});

test('delete shows the preview and waits for the exact name', async ({ page }) => {
  await page.setViewportSize({ width: 390, height: 844 });
  await install(page);
  await page.goto('/settings/server/users');
  await page.getByRole('button', { name: 'Manage byron' }).click();
  await page.getByRole('button', { name: 'Delete account' }).click();
  const sheet = page.getByRole('dialog', { name: 'Delete account' });
  await expect(sheet.getByRole('heading', { name: 'Delete byron?' })).toBeVisible();
  await expect(sheet.getByText('Deletes 3 books, 2 that others can read.')).toBeVisible();
  await expect(sheet.getByText('1 book stays with the person who also has it.')).toBeVisible();
  const remove = sheet.getByRole('button', { name: 'Delete', exact: true });
  const confirm = sheet.getByRole('textbox', { name: 'Type byron to confirm' });
  await expect(remove).toBeDisabled();
  for (const typed of ['byro', 'Byron', 'byron ']) {
    await confirm.fill(typed);
    await expect(remove).toBeDisabled();
  }
  await confirm.fill('byron');
  await expect(remove).toBeEnabled();
  const reload = page.waitForRequest((request) =>
    request.method() === 'GET' && request.url().endsWith('/v1/admin/users'));
  await remove.click();
  await reload;
  await expect(sheet).toHaveCount(0);
  await expect(page.getByRole('button', { name: '@byron' })).toHaveCount(0);
  await expect(page.getByRole('status')).toContainText('Deleted byron.');
});

test('a delete conflict stays inline in the sheet', async ({ page }) => {
  await page.setViewportSize({ width: 390, height: 844 });
  const state = await install(page);
  state.deleteError = 'You can\'t delete your own account.';
  await page.goto('/settings/server/users');
  await page.getByRole('button', { name: 'Manage byron' }).click();
  await page.getByRole('button', { name: 'Delete account' }).click();
  const sheet = page.getByRole('dialog', { name: 'Delete account' });
  await sheet.getByRole('textbox', { name: 'Type byron to confirm' }).fill('byron');
  await sheet.getByRole('button', { name: 'Delete', exact: true }).click();
  await expect(sheet.getByRole('alert')).toHaveText('You can\'t delete your own account.');
  await expect(sheet.getByRole('button', { name: 'Keep' })).toBeVisible();
  await sheet.getByRole('button', { name: 'Keep' }).click();
  await expect(page.getByRole('button', { name: '@byron' })).toBeVisible();
});

test('the account Password row changes the password', async ({ page }) => {
  await page.setViewportSize({ width: 390, height: 844 });
  await install(page);
  await page.goto('/settings/account');
  await page.getByRole('button', { name: 'Password' }).click();
  const sheet = page.getByRole('dialog', { name: 'Change password' });
  await sheet.getByLabel('Current').fill('nope-nope');
  await sheet.getByLabel('New').fill('short');
  let posted = 0;
  page.on('request', (request) => {
    if (request.url().endsWith('/v1/auth/password')) posted += 1;
  });
  await sheet.getByRole('button', { name: 'Save' }).click();
  await expect(sheet.getByRole('alert')).toHaveText('Password must be at least 8 characters.');
  expect(posted).toBe(0);
  await sheet.getByLabel('New').fill('password2');
  await sheet.getByRole('button', { name: 'Save' }).click();
  await expect(sheet.getByRole('alert')).toHaveText('Wrong password.');
  await sheet.getByLabel('Current').fill('password1');
  const change = page.waitForRequest((request) => request.url().endsWith('/v1/auth/password'));
  await sheet.getByRole('button', { name: 'Save' }).click();
  expect((await change).postDataJSON()).toEqual({
    currentPassword: 'password1',
    newPassword: 'password2',
  });
  await expect(sheet).toHaveCount(0);
  await expect(page.getByRole('status')).toContainText('Password changed. Other devices are signed out.');
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

test('a failed /v1/me is retried and the Server row appears', async ({ page }) => {
  await page.setViewportSize({ width: 390, height: 844 });
  await install(page, { meFails: 1 });
  const failed = page.waitForResponse((response) =>
    response.url().includes('/v1/me') && response.status() === 500);
  await page.goto('/settings');
  await failed;
  await expect(page.getByRole('button', { name: 'About' })).toBeVisible();
  await expect(page.getByRole('button', { name: 'Server' })).toHaveCount(0);
  const retried = page.waitForResponse((response) =>
    response.url().includes('/v1/me') && response.status() === 200);
  await page.getByRole('button', { name: 'About' }).click();
  await retried;
  await page.goBack();
  await expect(page.getByRole('button', { name: 'Server' })).toBeVisible();
});

test('blurring an unedited EPUB size does not patch', async ({ page }) => {
  await page.setViewportSize({ width: 390, height: 844 });
  await install(page, { maxEpubBytes: 5_000_000 });
  await page.goto('/settings/server');
  const input = page.locator('.epub-field input');
  await expect(input).toHaveValue('4.768');
  const patch = page.waitForRequest(
    (request) => request.method() === 'PATCH' && request.url().includes('/v1/admin/settings'),
    { timeout: 1000 },
  ).then(() => true, () => false);
  await input.focus();
  await input.blur();
  expect(await patch).toBe(false);
  await expect(input).toHaveValue('4.768');
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
      const state = await install(page, { theme });
      state.byron.disabled = true;
      await page.goto('/settings');
      await expect(page.getByRole('button', { name: 'Server' })).toBeVisible();
      await page.screenshot({ path: `${dir}/web-settings-${label}.png` });
      await page.getByRole('button', { name: 'Server' }).click();
      await expect(page.getByText('38.6 MB')).toBeVisible();
      await page.screenshot({ path: `${dir}/web-settings-server-${label}.png` });
      await page.getByRole('button', { name: 'Users' }).click();
      await expect(page.getByRole('button', { name: 'ada admin' })).toBeVisible();
      await page.screenshot({ path: `${dir}/web-settings-server-users-${label}.png` });
      await page.getByRole('button', { name: 'Manage byron' }).click();
      await expect(page.getByRole('button', { name: 'Enable' })).toBeVisible();
      await page.screenshot({ path: `${dir}/web-account-sheet-${label}.png` });
      await page.getByRole('button', { name: 'Reset password' }).click();
      await page.screenshot({ path: `${dir}/web-account-reset-confirm-${label}.png` });
      await page.getByRole('button', { name: 'Reset', exact: true }).click();
      await expect(page.getByRole('button', { name: 'Copy' })).toBeVisible();
      await page.screenshot({ path: `${dir}/web-account-reset-done-${label}.png` });
      await page.getByRole('button', { name: 'Done' }).click();
      await page.getByRole('button', { name: 'Manage byron' }).click();
      await page.getByRole('button', { name: 'Delete account' }).click();
      await expect(page.getByText('Deletes 3 books, 2 that others can read.')).toBeVisible();
      await page.getByRole('textbox', { name: 'Type byron to confirm' }).fill('byron');
      await page.screenshot({ path: `${dir}/web-account-delete-${label}.png` });
      await page.getByRole('button', { name: 'Keep' }).click();
      await page.goto('/settings/account');
      await page.getByRole('button', { name: 'Password' }).click();
      await expect(page.getByRole('dialog', { name: 'Change password' })).toBeVisible();
      await page.screenshot({ path: `${dir}/web-change-password-${label}.png` });
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
