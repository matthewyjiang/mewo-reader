import { expect, test } from '@playwright/test';
import type { Page } from '@playwright/test';

// Synthetic cached books isolate background shelf/feed updates from the network.
async function shelf(page: Page) {
  await page.goto('/search');
  await page.evaluate(async () => {
    const url = performance.getEntriesByType('resource')
      .find((entry) => entry.name.includes('/src/lib/store.svelte.ts'))!.name;
    const { store } = await import(url);
    const book = { id: 'loaded', title: 'Fixture', author: 'Author', handle: 'author',
      importedAt: 1, postCount: 2000, progressIndex: 0, isSample: false,
      mine: true, shared: false, ownerUsername: 'fixture' };
    store.api.library = async () => [{ ...book, id: 'pending', importedAt: 2 }, book];
    store.api.cover = async () => null;
    store.api.likes = async () => [];
    store.api.commentIndex = async () => ({ counts: {}, mine: [] });
    store.session = { token: 'fixture', username: 'fixture' };
    store.feeds = { loaded: Array.from({ length: 2000 }, (_, i) => ({
      id: `p${i}`, kind: 'Body', text: `The fixture passage number ${i}.`, chapter: 'One',
    })) };
  });
  await page.getByRole('searchbox').fill('the');
  await expect(page.locator('article.post').first()).toBeVisible();
}

async function visibleRow(page: Page) {
  return page.evaluate(() => {
    const post = [...document.querySelectorAll('article.post')]
      .find((row) => row.getBoundingClientRect().bottom > 56)!;
    return { href: post.querySelector('a.post-body')!.getAttribute('href'),
      top: post.getBoundingClientRect().top };
  });
}

test('a metadata update retains the result control and keyboard focus', async ({ page }) => {
  await shelf(page);
  const button = page.locator('article.post').first().getByRole('button').first();
  const original = await button.elementHandle();
  await button.focus();
  await page.evaluate(async () => {
    const url = performance.getEntriesByType('resource')
      .find((entry) => entry.name.includes('/src/lib/store.svelte.ts'))!.name;
    const { store } = await import(url);
    const book = store.books.find((book: { id: string }) => book.id === 'loaded');
    store.replaceBook({ ...book, author: 'Updated Author', progressIndex: 20 });
  });
  await expect(page.locator('article.post').first().locator('.post-name')).toHaveText('Updated Author');
  expect(await original!.evaluate((node) => node.isConnected)).toBe(true);
  await expect(button).toBeFocused();
});

test('an arriving feed retains the passage, offset, DOM node, and focus', async ({ page }) => {
  await shelf(page);
  await page.locator('main').evaluate(async (main) => {
    main.scrollTop = 20000;
    await new Promise(requestAnimationFrame);
    await new Promise(requestAnimationFrame);
  });
  const located = await visibleRow(page);
  const post = page.locator(`a.post-body[href="${located.href}"]`).locator('xpath=ancestor::article');
  const original = await post.elementHandle();
  const button = post.getByRole('button').first();
  await button.focus();
  const before = await visibleRow(page);
  const index = await post.evaluate((node) => Number(node.closest<HTMLElement>('[data-post-index]')!.dataset.postIndex));
  await page.evaluate(async () => {
    const url = performance.getEntriesByType('resource')
      .find((entry) => entry.name.includes('/src/lib/store.svelte.ts'))!.name;
    const { store } = await import(url);
    store.feeds = { ...store.feeds, pending: Array.from({ length: 50 }, (_, i) => ({
      id: `new${i}`, kind: 'Body', text: `The arriving passage ${i}.`, chapter: 'One',
    })) };
  });
  await expect(post.locator('..')).toHaveAttribute('data-post-index', String(index + 50));
  const after = await visibleRow(page);
  expect(after.href).toBe(before.href);
  // scrollTop rounds to a device pixel in some browsers.
  expect(Math.abs(after.top - before.top)).toBeLessThan(1);
  expect(await original!.evaluate((node) => node.isConnected)).toBe(true);
  await expect(button).toBeFocused();
});
