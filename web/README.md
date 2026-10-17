# Web

The hosted shelf in a browser. Svelte 5, TypeScript, Vite. The Rust server parses books and owns `/v1`. The page uses the current origin, so there is no server URL field and no local shelf.

Version strings baked into the bundle: [root README](../README.md#version-and-release). API and `MEWO_WEB_DIR`: [server/README.md](../server/README.md).

## Development

Node 22 (`mise.toml`). `web/package.json` allows `^20.19.0` or `>=22.12.0`. Node 21 is unsupported.

Start the server from `server/` (`cargo run`), then:

```bash
cd web
npm ci
npm run dev
```

Open the URL Vite prints. It proxies `/v1` to `http://localhost:8787`. If signup is disabled, the server's error is shown as-is.

## Check and build

Tests use Node 22 or newer for its built-in TypeScript support.

```bash
npm test
npm run check
npm run build
```

Browser tests check keyboard focus and scroll when background updates change search results:

```bash
npx playwright install chromium
npm run test:browser
```

The server-image workflow runs these tests in Chromium as well as the unit tests and type checks.

Vite writes `web/dist`. `MEWO_VERSION` and `MEWO_COMMIT`, when set for `npm run build`, are the version and short commit under Settings → About. When unset, the build uses `git describe` and `git rev-parse --short=7 HEAD`.

Serve that build from the API:

```bash
# from server/
MEWO_WEB_DIR=../web/dist cargo run
```

Client routes are `/`, `/books/:id` (optional `?post=`), `/profiles/:username`, `/search`, `/likes`, and `/settings`, `/settings/account`, `/settings/display`, `/settings/library`, `/settings/about`. The server falls back to `index.html` for non-API paths. Missing `/v1` routes stay JSON.

## Behavior

- Settings → About shows this bundle, `GET /v1/version`, and the page origin.
- The shelf (title, author, handle, progress, mine, shared, owner) is saved in localStorage per page origin and username. Home paints that list immediately and refreshes it in the background. "No books" is shown only after a refresh that returned zero books. A failed refresh keeps the saved list and shows a notice. With no saved list yet, Home shows a spinner instead.
- Posts are saved in IndexedDB for that same origin and username and reused until sign-out, including after a restart. Likes and profiles load the feeds they need, including lines liked on a phone.
- Pull down from the top of Home on a touch screen to refresh. The shelf slides down with the finger and the spinner sits in the gap under the top bar. While the request runs, that gap stays open, then the shelf slides back. A mouse gets a refresh button in the top bar. Reduced motion skips the slide.
- Quote shares the passage plus `— author, title`. Share sends the passage alone. Both use Web Share, then the clipboard.
- Keyboard use keeps the chrome visible. Settings → Display → Always show navigation keeps it visible. Reduced motion and safe-area insets are respected.
- Account settings show the signed-in username and a link to that profile. There is no local name/handle editor. Sign-out clears the saved shelf and posts for that account.
