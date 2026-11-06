import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { test } from 'node:test';
import { fileURLToPath } from 'node:url';
import { inflateSync } from 'node:zlib';

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

function readPng(buf) {
  assert.ok(
    buf.subarray(0, 8).equals(Buffer.from([137, 80, 78, 71, 13, 10, 26, 10])),
  );
  let offset = 8;
  let width = 0;
  let height = 0;
  let bitDepth = 0;
  let colorType = 0;
  let interlace = 0;
  const idat = [];
  while (offset + 8 <= buf.length) {
    const length = buf.readUInt32BE(offset);
    const type = buf.toString('ascii', offset + 4, offset + 8);
    const data = buf.subarray(offset + 8, offset + 8 + length);
    if (type === 'IHDR') {
      width = data.readUInt32BE(0);
      height = data.readUInt32BE(4);
      bitDepth = data[8];
      colorType = data[9];
      interlace = data[12];
    } else if (type === 'IDAT') {
      idat.push(data);
    } else if (type === 'IEND') {
      break;
    }
    offset += 12 + length;
  }
  return {
    width,
    height,
    bitDepth,
    colorType,
    interlace,
    raw: inflateSync(Buffer.concat(idat)),
  };
}

function rgbaRows(png) {
  assert.equal(png.bitDepth, 8);
  assert.equal(png.colorType, 6);
  assert.equal(png.interlace, 0);
  const bpp = 4;
  const stride = png.width * bpp;
  const rows = [];
  let i = 0;
  let prev = Buffer.alloc(stride);
  for (let y = 0; y < png.height; y++) {
    const filter = png.raw[i++];
    const row = Buffer.from(png.raw.subarray(i, i + stride));
    i += stride;
    for (let x = 0; x < stride; x++) {
      const left = x >= bpp ? row[x - bpp] : 0;
      const up = prev[x];
      const upLeft = x >= bpp ? prev[x - bpp] : 0;
      let pred = 0;
      if (filter === 1) pred = left;
      else if (filter === 2) pred = up;
      else if (filter === 3) pred = (left + up) >> 1;
      else if (filter === 4) {
        const paeth = left + up - upLeft;
        const pa = Math.abs(paeth - left);
        const pb = Math.abs(paeth - up);
        const pc = Math.abs(paeth - upLeft);
        pred = pa <= pb && pa <= pc ? left : pb <= pc ? up : upLeft;
      } else if (filter !== 0) {
        throw new Error(`png filter ${filter}`);
      }
      row[x] = (row[x] + pred) & 255;
    }
    rows.push(row);
    prev = row;
  }
  assert.equal(i, png.raw.length);
  return rows;
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

test('the badge is hosted in the repo and shown at 161x48', () => {
  assert.equal(readme.includes('raw.githubusercontent.com/ImranR98/Obtainium'), false);
  assert.match(
    readme,
    /<img src="docs\/assets\/badge_obtainium\.png" width="161" height="48" alt="Get it on Obtainium">/,
  );
  assert.match(
    readme,
    /<a href="https:\/\/github\.com\/matthewyjiang\/mewo-reader\/releases\/latest">Download the APK<\/a>/,
  );
  assert.equal((readme.match(/^## Install$/gm) ?? []).length, 1);
  assert.match(readme, /Obtainium keeps the app updated automatically\./);

  const png = readPng(readFileSync(join(root, 'docs/assets/badge_obtainium.png')));
  const rows = rgbaRows(png);
  const alpha = (x, y) => rows[y][x * 4 + 3];
  const edgeHasInk = (xs, ys) => xs.some((x) => ys.some((y) => alpha(x, y) !== 0));
  const xs = [...Array(png.width).keys()];
  const ys = [...Array(png.height).keys()];
  assert.equal(edgeHasInk(xs, [0]), true);
  assert.equal(edgeHasInk(xs, [png.height - 1]), true);
  assert.equal(edgeHasInk([0], ys), true);
  assert.equal(edgeHasInk([png.width - 1], ys), true);
  assert.equal(png.width, 564);
  assert.equal(png.height, 168);
});
