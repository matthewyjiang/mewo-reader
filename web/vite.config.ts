import { execFileSync } from 'node:child_process';
import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
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

const root = fileURLToPath(new URL('.', import.meta.url));

function gitOutput(args: string[]): string | undefined {
  try {
    const out = execFileSync('git', args, { cwd: root, encoding: 'utf8' }).trim();
    return out || undefined;
  } catch {
    return undefined;
  }
}

/** `git describe --tags --always --dirty`, with one leading `v` removed. */
function gitVersion(): string | undefined {
  const described = gitOutput(['describe', '--tags', '--always', '--dirty']);
  if (!described) return undefined;
  return described.startsWith('v') ? described.slice(1) : described;
}

function gitCommit(): string | undefined {
  return gitOutput(['rev-parse', '--short=7', 'HEAD']);
}

const version = nonempty(process.env.MEWO_VERSION) ?? gitVersion() ?? packageVersion();
const commit = nonempty(process.env.MEWO_COMMIT) ?? gitCommit() ?? null;

export default defineConfig({
  plugins: [svelte()],
  define: {
    __MEWO_VERSION__: JSON.stringify(version),
    __MEWO_COMMIT__: JSON.stringify(commit),
  },
  server: { proxy: { '/v1': 'http://localhost:8787' } },
  build: { outDir: 'dist' },
});
