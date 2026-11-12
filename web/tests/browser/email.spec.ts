import { expect, test, type Page, type Route } from '@playwright/test';
import { mkdir } from 'node:fs/promises';

type Theme = 'x_light' | 'x_lights_out';

function freshState(passwordReset: boolean) {
  return {
    passwordReset,
    me: {
      username: 'ada',
      isAdmin: true,
      canRequest: false,
      email: 'ada@example.com' as string | null,
      pendingEmail: null as string | null,
      passwordReset,
    },
    settings: {
      allowSignup: true,
      maxEpubBytes: 64 * 1024 * 1024,
      languages: ['en'],
      supportedLanguages: [{ code: 'en', name: 'English' }],
      publicUrl: 'https://mewo.example',
      smtpHost: 'smtp.example.com',
      smtpPort: 587,
      smtpTls: 'starttls',
      smtpUsername: 'mewo',
      mailFrom: 'Mewo <mewo@example.com>',
      secrets: { prowlarrApiKey: false, sabnzbdApiKey: false, smtpPassword: true },
    },
    bodies: [] as { path: string; method: string; body: unknown }[],
  };
}

type State = ReturnType<typeof freshState>;

async function install(
  page: Page,
  options: { passwordReset?: boolean; signedIn?: boolean; theme?: Theme } = {},
): Promise<State> {
  const state = freshState(options.passwordReset ?? true);
  await page.addInitScript(({ signedIn, theme }) => {
    if (signedIn) {
      localStorage.setItem('mewo.session', JSON.stringify({ token: 'token', username: 'ada' }));
    }
    localStorage.setItem('mewo.display', JSON.stringify({
      theme, step: 2, mewo: false, keepBars: true,
    }));
  }, { signedIn: options.signedIn ?? false, theme: options.theme ?? 'x_light' });
  await page.route('**/v1/**', (route) => answer(route, state));
  return state;
}

async function answer(route: Route, state: State) {
  const request = route.request();
  const path = new URL(request.url()).pathname;
  const method = request.method();
  const body = request.postData() ? request.postDataJSON() : undefined;
  if (body !== undefined) state.bodies.push({ path, method, body });
  const json = (value: unknown, status = 200) => route.fulfill({
    status,
    contentType: 'application/json',
    body: JSON.stringify(value),
  });
  if (path === '/v1/instance') {
    return json({ allowSignup: true, passwordReset: state.passwordReset });
  }
  if (path === '/v1/auth/reset/request') return route.fulfill({ status: 202, body: '' });
  if (path === '/v1/auth/reset') {
    if ((body as { token: string }).token === 'good') return route.fulfill({ status: 204 });
    return json({ error: 'This link has expired.' }, 400);
  }
  if (path === '/v1/auth/email/verify') {
    if ((body as { token: string }).token === 'good') return json({ email: 'ada@example.com' });
    return json({ error: 'This link has expired.' }, 400);
  }
  if (path === '/v1/auth/register') return json({ token: 'token', username: 'ada' });
  if (path === '/v1/me/email' && method === 'PUT') {
    const change = body as { email: string | null; currentPassword: string };
    if (change.currentPassword !== 'password1') return json({ error: 'Wrong password.' }, 403);
    if (change.email === null) {
      state.me.email = null;
      state.me.pendingEmail = null;
      return route.fulfill({ status: 204 });
    }
    state.me.pendingEmail = change.email;
    return route.fulfill({ status: 202 });
  }
  if (path === '/v1/me') return json(state.me);
  if (path === '/v1/library') return json({ books: [] });
  if (path === '/v1/feed') return json({ items: [], nextCursor: null });
  if (path === '/v1/admin/settings' && method === 'PATCH') {
    const patch = body as Record<string, unknown>;
    for (const [key, value] of Object.entries(patch)) {
      if (key === 'smtpPassword') state.settings.secrets.smtpPassword = value !== null;
      else Object.assign(state.settings, { [key]: value });
    }
    return json(state.settings);
  }
  if (path === '/v1/admin/settings') return json(state.settings);
  if (path === '/v1/admin/server') {
    return json({ users: 1, books: 0, comments: 0, reposts: 0, storageBytes: 0, invites: 0 });
  }
  if (path === '/v1/admin/mail/test') return json({ name: 'mail', ok: true, detail: 'Sent' });
  return json({ error: 'Not found.' }, 404);
}

test('Forgot password and the signup email are hidden when the server cannot mail', async ({ page }) => {
  await page.setViewportSize({ width: 390, height: 844 });
  await install(page, { passwordReset: false });
  await page.goto('/');
  await expect(page.getByRole('heading', { name: 'Sign in' })).toBeVisible();
  await expect(page.getByRole('button', { name: 'Create an account' })).toBeVisible();
  await expect(page.getByRole('button', { name: 'Forgot password?' })).toHaveCount(0);
  await page.getByRole('button', { name: 'Create an account' }).click();
  await expect(page.getByRole('heading', { name: 'Create account' })).toBeVisible();
  await expect(page.getByLabel('Email (optional)')).toHaveCount(0);
});

test('Forgot password sends a link and says the same thing for any account', async ({ page }) => {
  await page.setViewportSize({ width: 390, height: 844 });
  const state = await install(page);
  await page.goto('/');
  await page.getByLabel('Username').fill('ada');
  await page.getByRole('button', { name: 'Forgot password?' }).click();
  await expect(page.getByRole('heading', { name: 'Reset password' })).toBeVisible();
  await expect(page.getByLabel('Username or email')).toHaveValue('ada');
  await page.getByLabel('Username or email').fill('nobody@example.com');
  await page.getByRole('button', { name: 'Send link' }).click();
  await expect(page.getByText('If that account has an email, we sent a link.')).toBeVisible();
  expect(state.bodies).toContainEqual({
    path: '/v1/auth/reset/request',
    method: 'POST',
    body: { login: 'nobody@example.com' },
  });
  await page.getByRole('button', { name: 'Sign in instead' }).click();
  await expect(page.getByRole('heading', { name: 'Sign in' })).toBeVisible();
});

test('signup shows the optional email and sends it', async ({ page }) => {
  await page.setViewportSize({ width: 390, height: 844 });
  const state = await install(page);
  await page.goto('/');
  await page.getByRole('button', { name: 'Create an account' }).click();
  await expect(page.getByLabel('Email (optional)')).toBeVisible();
  await page.getByLabel('Username').fill('ada');
  await page.getByLabel('Password', { exact: true }).fill('password1');
  await page.getByLabel('Email (optional)').fill('ada@example.com');
  await page.getByRole('button', { name: 'Create account' }).click();
  await expect.poll(() => state.bodies.find((row) => row.path === '/v1/auth/register')?.body)
    .toEqual({ username: 'ada', password: 'password1', email: 'ada@example.com' });
});

test('a reset link sets the password and clears the token from the URL', async ({ page }) => {
  await page.setViewportSize({ width: 390, height: 844 });
  const state = await install(page);
  await page.goto('/reset#token=good');
  await expect(page.getByRole('heading', { name: 'Reset password' })).toBeVisible();
  await expect.poll(() => page.url()).toBe('http://127.0.0.1:4173/reset');
  await page.getByLabel('New password').fill('short');
  await expect(page.getByText('At least 8 characters.')).toBeVisible();
  await expect(page.getByRole('button', { name: 'Set password' })).toBeDisabled();
  await page.getByLabel('New password').fill('newpassword');
  await page.getByRole('button', { name: 'Set password' }).click();
  await expect(page.getByRole('heading', { name: 'Password changed' })).toBeVisible();
  expect(state.bodies).toContainEqual({
    path: '/v1/auth/reset',
    method: 'POST',
    body: { token: 'good', newPassword: 'newpassword' },
  });
  await page.getByRole('button', { name: 'Sign in' }).click();
  await expect(page.getByRole('heading', { name: 'Sign in' })).toBeVisible();
});

test('an expired reset link says so, even with a session', async ({ page }) => {
  await page.setViewportSize({ width: 390, height: 844 });
  await install(page, { signedIn: true });
  await page.goto('/reset#token=old');
  await page.getByLabel('New password').fill('newpassword');
  await page.getByRole('button', { name: 'Set password' }).click();
  await expect(page.getByRole('heading', { name: 'Link expired' })).toBeVisible();
  await expect(page.getByRole('button', { name: 'Sign in' })).toBeVisible();
  await page.goto('/reset');
  await expect(page.getByRole('heading', { name: 'Link expired' })).toBeVisible();
});

test('a verify link confirms the email', async ({ page }) => {
  await page.setViewportSize({ width: 390, height: 844 });
  await install(page, { signedIn: true });
  await page.goto('/verify-email#token=good');
  await expect(page.getByRole('heading', { name: 'Email confirmed' })).toBeVisible();
  await expect(page.getByText('ada@example.com')).toBeVisible();
  expect(page.url()).toBe('http://127.0.0.1:4173/verify-email');
  await page.getByRole('button', { name: 'Open Mewo' }).click();
  await expect(page.getByRole('tab', { name: 'Feed' })).toBeVisible();
});

test('the Email sheet saves and removes the address', async ({ page }) => {
  await page.setViewportSize({ width: 390, height: 844 });
  const state = await install(page, { signedIn: true });
  await page.goto('/settings/account');
  const row = page.getByRole('button', { name: /^Email/ });
  await expect(row).toContainText('ada@example.com');
  await row.click();
  const sheet = page.getByRole('dialog', { name: 'Email' });
  await expect(sheet.getByLabel('Email', { exact: true })).toHaveValue('ada@example.com');
  await sheet.getByLabel('Email', { exact: true }).fill('new@example.com');
  await sheet.getByLabel('Current password').fill('wrong-pass');
  await sheet.getByRole('button', { name: 'Save' }).click();
  await expect(sheet.getByText('Wrong password.')).toBeVisible();
  await sheet.getByLabel('Current password').fill('password1');
  await sheet.getByRole('button', { name: 'Save' }).click();
  await expect(page.getByText('Check new@example.com for a link.')).toBeVisible();
  await expect(row).toContainText('Check inbox');

  await row.click();
  await sheet.getByLabel('Current password').fill('password1');
  await sheet.getByRole('button', { name: 'Remove email' }).click();
  await expect(row).toContainText('Add');
  expect(state.bodies.at(-1)).toEqual({
    path: '/v1/me/email',
    method: 'PUT',
    body: { email: null, currentPassword: 'password1' },
  });
});

test('the Email row is hidden when the server cannot mail', async ({ page }) => {
  await page.setViewportSize({ width: 390, height: 844 });
  await install(page, { signedIn: true, passwordReset: false });
  await page.goto('/settings/account');
  await expect(page.getByRole('button', { name: 'Password' })).toBeVisible();
  await expect(page.getByRole('button', { name: /^Email/ })).toHaveCount(0);
});

test('the Mail page shows a saved password and the test result', async ({ page }) => {
  await page.setViewportSize({ width: 390, height: 844 });
  const state = await install(page, { signedIn: true });
  await page.goto('/settings/server');
  const mail = page.getByRole('button', { name: /^Mail/ });
  await expect(mail).toContainText('On');
  await mail.click();
  await expect(page.getByLabel('SMTP host')).toHaveValue('smtp.example.com');
  await expect(page.getByLabel('Password', { exact: true })).toHaveAttribute('placeholder', 'Saved');
  await expect(page.getByLabel('Public URL')).toHaveAttribute('placeholder', 'http://127.0.0.1:4173');
  await page.getByLabel('Security').selectOption('tls');
  await expect.poll(() => state.settings.smtpTls).toBe('tls');
  await page.getByLabel('Send test to').fill('me@example.com');
  await page.getByRole('button', { name: 'Send', exact: true }).click();
  await expect(page.locator('.fact-row')).toContainText('Sent');
  await page.getByRole('button', { name: 'Clear SMTP password' }).click();
  await expect.poll(() => state.settings.secrets.smtpPassword).toBe(false);
  await expect(page.getByLabel('Password', { exact: true })).toHaveAttribute('placeholder', '');
});

test('shots of email and password reset', async ({ browser }) => {
  test.skip(!process.env.MEWO_SHOTS, 'set MEWO_SHOTS=1 to write email screenshots');
  const dir = process.env.MEWO_SHOTS_DIR ?? 'test-results/email-shots';
  await mkdir(dir, { recursive: true });
  for (const theme of ['x_light', 'x_lights_out'] as const) {
    for (const width of [390, 1280]) {
      const label = `${width}-${theme === 'x_light' ? 'light' : 'dark'}`;
      const context = await browser.newContext({
        viewport: { width, height: width === 390 ? 844 : 900 },
        colorScheme: theme === 'x_light' ? 'light' : 'dark',
      });
      try {
        const page = await context.newPage();
        await install(page, { theme });
        await page.goto('/');
        await expect(page.getByRole('button', { name: 'Forgot password?' })).toBeVisible();
        await page.screenshot({ path: `${dir}/web-sign-in-forgot-${label}.png` });
        await page.getByRole('button', { name: 'Create an account' }).click();
        await expect(page.getByLabel('Email (optional)')).toBeVisible();
        await page.screenshot({ path: `${dir}/web-sign-up-email-${label}.png` });
        await page.getByRole('button', { name: 'Sign in instead' }).click();
        await page.getByRole('button', { name: 'Forgot password?' }).click();
        await page.getByLabel('Username or email').fill('ada');
        await page.getByRole('button', { name: 'Send link' }).click();
        await expect(page.getByText('If that account has an email, we sent a link.')).toBeVisible();
        await page.screenshot({ path: `${dir}/web-reset-sent-${label}.png` });
        await page.goto('/reset#token=good');
        await page.getByLabel('New password').fill('newpassword');
        await page.screenshot({ path: `${dir}/web-reset-form-${label}.png` });
        await page.getByRole('button', { name: 'Set password' }).click();
        await expect(page.getByRole('heading', { name: 'Password changed' })).toBeVisible();
        await page.screenshot({ path: `${dir}/web-reset-done-${label}.png` });

        const signed = await context.newPage();
        await install(signed, { theme, signedIn: true });
        await signed.goto('/settings/account');
        await expect(signed.getByRole('button', { name: /^Email/ })).toBeVisible();
        await signed.screenshot({ path: `${dir}/web-account-email-${label}.png` });
        await signed.getByRole('button', { name: /^Email/ }).click();
        await expect(signed.getByRole('dialog', { name: 'Email' })).toBeVisible();
        await signed.screenshot({ path: `${dir}/web-email-sheet-${label}.png` });
        await signed.goto('/settings/server/mail');
        await signed.getByLabel('Send test to').fill('me@example.com');
        await signed.getByRole('button', { name: 'Send', exact: true }).click();
        await expect(signed.locator('.fact-row')).toContainText('Sent');
        await signed.screenshot({ path: `${dir}/web-mail-${label}.png` });
      } finally {
        await context.close();
      }
    }
  }
});
