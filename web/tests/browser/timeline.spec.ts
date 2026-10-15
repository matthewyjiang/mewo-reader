import { expect, test } from '@playwright/test';

test('an explicit jump supersedes the older queued dataset restoration', async ({ page }) => {
  await page.goto('/search');
  const anchor = await page.evaluate(async () => {
    const fixture = await import('/tests/browser/timeline-fixture.ts');
    return fixture.updateAndJump();
  });
  expect(anchor.key).toBe('24');
  expect(anchor.index).toBe(25);
  expect(anchor.offset).toBe(0);
});
