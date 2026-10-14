# Mewo web

The hosted shelf, in a browser. Svelte 5 runes, strict TypeScript, and Vite; no UI kit, router dependency, or EPUB parser. The Rust server parses books and owns `/v1`. The web app uses the current origin, so there is no server URL field and no local shelf.

## Development

Use Node **20.19+** or **22.12+** (Vite 8's requirement; Node 21 is not supported).

1. Start the Rust server on port 8787, from `server/`:

   ```sh
   cargo run
   ```

2. From `web/`:

   ```sh
   npm ci
   npm run dev
   ```

Open the URL printed by Vite. Its dev server proxies `/v1` to `http://localhost:8787`, keeping browser requests same-origin. Sign in or create an account; if server signup is disabled, its message is shown directly.

## Check and build

```sh
npm run check
npm run build
```

`MEWO_VERSION` and `MEWO_COMMIT`, when set for `npm run build`, are the version and short commit shown under Settings → About. When they are unset, the build uses `git describe --tags --always --dirty` (one leading `v` removed) and `git rev-parse --short=7 HEAD`. If git is unavailable, the version is `package.json` and the commit is omitted. The Docker image passes the same args the server binary gets.

Vite writes the production site to `web/dist`. For local same-origin serving, start the Rust server with the built directory configured:

```sh
# From server/
MEWO_WEB_DIR=../web/dist cargo run
```

The server serves assets and `index.html` at `/`, with history-route fallback for `/books/:id`, `/profiles/:username`, `/search`, `/likes`, and `/settings/*`. API requests remain under `/v1`; missing API routes must not fall through to the SPA. The Docker image builds and serves these assets with the API.

## Implementation notes

- `src/lib/api.ts`: typed same-origin bearer client. `ApiError` carries HTTP status and the server's `{ "error": "…" }` message. Uploads use XHR for progress; authenticated cover requests become cached blob URLs. Every 401 expires the session.
- `src/lib/store.svelte.ts`: hosted shelf, store-owned serialized social mutations, and per-book immutable in-memory post caches. Resource loading is concurrent; removed books and old sessions cannot publish late responses or allocate cover URLs.
- `src/lib/progress.ts`: debounced, serialized reading-place writes. The latest unacknowledged value stays pending and is persisted per account in localStorage, retried on the next save/flush, and replayed on the next signed-in session. Navigation flushes writes; pagehide and hidden visibility send the latest value immediately with fetch keepalive, even during an older request. Signing out clears in-memory private caches but retains any unacknowledged reading place for that account.
- `src/lib/router.svelte.ts`: history-API navigation, per-entry reader anchors (post index plus offset within the post), and pixel scroll positions for smaller screens. Switching tabs restores each tab's last position. Entering Search through a tab focuses its field; browser Back does not force search-field focus. Links have real destinations; loading a deep link resumes that book, or focuses its `?post=` passage.
- `src/lib/preferences.svelte.ts` and `src/app.css`: the four phone palettes, Atkinson Hyperlegible, seven phone post-font steps, and localStorage display preferences. Global CSS contains resets, tokens, typography, and shared primitives; screen/component-specific styles live with their Svelte owners. Chrome does not scale with post font size.
- `src/components/`: accessible native-dialog sheets, post/thread rows, authentication and display controls. Escape dismisses sheets and focus returns to the invoking control.
- `src/screens/`: Home, Reader, Search, Likes, Profile and nested Settings destinations, including About. Each owns its loading, scrolling and route-specific presentation. About shows the web build, `GET /v1/version`, and the page origin.
- `src/App.svelte`: the shared shell, routing, account drawer, uploads and notices. Phone navigation becomes a simple left rail on wider screens, with a centered 600px timeline.

The reader uses `components/Timeline.svelte`, a variable-height virtual list with top/bottom spacers and one viewport of overscan on either side. ResizeObserver measures mounted rows, an index-keyed height cache remembers them, and a prefix-sum tree resolves offsets and jumps without scanning the whole book. Unmeasured rows use the previous four-line-plus-chrome height estimate. Only nearby posts create DOM, components, and effects; the end card is a virtual row too. The chapter sheet uses the same windowing, so even heading-heavy books stay bounded. Chapter, focused-post, saved-progress, and history jumps mount and measure their destination before final alignment. Reading place is the top-visible post, captured synchronously when leaving or backgrounding the reader. No virtual-list dependency or IndexedDB is required.

Posts are cached only for this browser session. **Post search does not survive browser restarts:** without a persistent feed cache, previously opened books must be loaded again before their passages appear in Search. This is an accepted web-scope difference from the phones. Search refreshes likes/comment indexes for books with post hits; Likes and profiles load required feeds, including passages liked on a phone.

Quote uses the same behavior as the phones: share the passage followed by `— author, title`. Share sends the passage alone. Both prefer Web Share and fall back to copying; plain-HTTP LAN origins have a legacy clipboard fallback.

Browsers do not expose a reliable screen-reader-active flag. Keyboard navigation keeps the chrome visible, and **Settings → Display → Always show navigation** provides an explicit persistent option. Reduced motion, safe-area insets, and the keyboard-resizing viewport are respected.

## Assets and scope

The two Atkinson Hyperlegible font files are copied from `ios/Mewo/Resources/Fonts`. `public/sample.epub` is the same short public-domain sample bundled by Android. The optional Teddy header uses `MewoTeddy.imageset/mewo_teddy.png` from iOS; it changes the web header, not an OS launcher icon.

Hosted usernames are server-managed; web Account settings show the signed-in identity and link to its profile rather than offering the phones' local-only name/handle editor. There is deliberately no offline/local shelf, PWA launcher integration, or persistent post cache.
