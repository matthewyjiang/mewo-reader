import assert from 'node:assert/strict';
import { existsSync, readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { test } from 'node:test';
import { fileURLToPath } from 'node:url';

const root = join(dirname(fileURLToPath(import.meta.url)), '..');
const readme = readFileSync(join(root, 'README.md'), 'utf8');

// Same decode the Obtainium redirect page uses: searchParams, then
// decodeURIComponent of the obtainium://app/ path, then JSON.parse.
function appFromReadme(markdown) {
  const href = markdown.match(
    /href="(https:\/\/apps\.obtainium\.imranr\.dev\/redirect\?r=[^"]+)"/,
  )?.[1];
  assert.ok(href, 'install badge links through the Obtainium redirector');
  const redirectUrl = new URL(href).searchParams.get('r');
  assert.ok(redirectUrl?.startsWith('obtainium://app/'));
  const encoded = redirectUrl.slice('obtainium://app/'.length);
  const app = JSON.parse(decodeURIComponent(encoded));
  const rebuilt =
    'https://apps.obtainium.imranr.dev/redirect?r=obtainium://app/' +
    encodeURIComponent(JSON.stringify(app));
  assert.equal(rebuilt, href);
  return app;
}

test('the install badge round-trips a full Obtainium app config', () => {
  const app = appFromReadme(readme);
  assert.equal(app.id, 'com.mewo.reader');
  assert.equal(app.url, 'https://github.com/matthewyjiang/mewo-reader');
  assert.equal(app.author, 'matthewyjiang');
  assert.equal(app.name, 'Mewo');
  assert.equal(typeof app.additionalSettings, 'string');
  const settings = JSON.parse(app.additionalSettings);
  assert.deepEqual(settings, { apkFilterRegEx: 'mewo-.*\\.apk' });
  const filter = new RegExp(settings.apkFilterRegEx);
  assert.equal(filter.test('mewo-0.1.21.apk'), true);
  assert.equal(filter.test('mewo-0.1.20.apk'), true);
  assert.equal(filter.test('app-release.apk'), false);
  assert.equal(filter.test('mewo-0.1.21.aab'), false);
});

test('the badge is a local file in the install section', () => {
  assert.equal(readme.includes('raw.githubusercontent.com/ImranR98/Obtainium'), false);
  assert.match(readme, /src="docs\/assets\/badge_obtainium\.png"/);
  assert.match(
    readme,
    /<a href="https:\/\/github\.com\/matthewyjiang\/mewo-reader\/releases\/latest">Download the APK<\/a>/,
  );
  assert.equal((readme.match(/^## Install$/gm) ?? []).length, 1);
  assert.equal(existsSync(join(root, 'docs/assets/badge_obtainium.png')), true);
});
