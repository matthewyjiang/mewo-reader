import { readFileSync } from 'node:fs';
import { defineConfig } from 'vite';
import { svelte } from '@sveltejs/vite-plugin-svelte';

function nonempty(value: string | undefined): string | undefined {
  const trimmed = value?.trim();
  return trimmed ? trimmed : undefined;
}

function packageVersion(): string {
  const parsed: unknown = JSON.parse(
    readFileSync(new URL('./package.json', import.meta.url), 'utf8'),
  );
  if (
    parsed && typeof parsed === 'object' && 'version' in parsed
    && typeof parsed.version === 'string' && parsed.version.length > 0
  ) {
    return parsed.version;
  }
  return '0.0.0';
}

const version = nonempty(process.env.MEWO_VERSION) ?? packageVersion();
const commit = nonempty(process.env.MEWO_COMMIT) ?? null;

export default defineConfig({
  plugins: [svelte()],
  define: {
    __MEWO_VERSION__: JSON.stringify(version),
    __MEWO_COMMIT__: JSON.stringify(commit),
  },
  server: { proxy: { '/v1': 'http://localhost:8787' } },
  build: { outDir: 'dist' },
});
