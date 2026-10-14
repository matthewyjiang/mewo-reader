# Server

Hosted library and optional web app for Mewo. Phones talk to the API through `HostedLibraryStore`.

Local and hosted are different shelves. This process is the hosted one. Books are a shared shelf. Likes and reading place stay per account. Notes on a line are public. The server parses hosted EPUBs into one canonical feed for Android, iOS, and the web app. Local phone books still use Readium.

## Run

Rust 1.88. From this directory:

```bash
cargo run
```

Listens on `0.0.0.0:8787`. Data lands in `./data` (`mewo.db` plus uploaded files).

On a phone on the same LAN, the server URL is `http://<that-machine>:8787`.

## Docker

From the repository root (the image builds both `server/` and `web/`):

```bash
docker build -f server/Dockerfile -t mewo-server .
docker run --rm -p 8787:8787 -v mewo-data:/data mewo-server
```

Same env vars as below. The image serves the built web app at `/` with `MEWO_WEB_DIR=/srv/web`. Keep `/data` on a volume or the next `docker run` starts with an empty shelf.

Pushes to `main` that touch `server/` or `web/` publish `ghcr.io/matthewyjiang/mewo-reader/server`. Tags: `latest`, `sha-<commit>`, and a semver tag if you push `v*`.

```bash
docker pull ghcr.io/matthewyjiang/mewo-reader/server:latest
docker run --rm -p 8787:8787 -v mewo-data:/data ghcr.io/matthewyjiang/mewo-reader/server:latest
```

The package is private until you flip it public under the repo's Packages tab. A 403 on pull means log in with a token that can read packages, or make the package public.

The process runs as uid 10001. If you bind-mount a host directory, that directory needs to be writable by that uid.

## Env

- `MEWO_LISTEN` default `0.0.0.0:8787`
- `MEWO_DATA_DIR` default `data`
- `MEWO_WEB_DIR` unset by default (API only). When set to an existing directory, serves its static files and falls back to `index.html` for client-side routes. Relative paths are relative to the process's working directory. Unknown `/v1/...` routes always return a JSON 404, never the web app. For a local web build, run from `server/` with `MEWO_WEB_DIR=../web/dist`.
- `MEWO_DATABASE_URL` default `sqlite://<data>/mewo.db?mode=rwc`
- `MEWO_ALLOW_SIGNUP` default on. Set `0` or `false` when the box faces the internet.
- `MEWO_MAX_EPUB_BYTES` default `67108864` (64 MiB)

Parsing reads at most `MAX_EPUB_TEXT_BYTES` (32 MiB) of inflated XML/XHTML per book. The most text measured is War and Peace (epub3.images) at 4_240_089 bytes, and a retail novel at 1_070_316, so 32 MiB is about 8x the biggest real book and stops a ZIP bomb before it becomes a DOM. Over budget is a 400 that names the budget, the limit, and the asked size. A missing or broken NCX is treated as no table of contents, like Readium.

The cap is 64 MiB because the bundled sample is 2_886 bytes, Gutenberg War and Peace (epub3.images) is 1_824_099, and illustrated Pride and Prejudice (epub3.images) is 24_835_578. That leaves room for a heavy illustrated novel and still rejects a dump. The HTTP body limit is that cap plus 4 MiB for a cover and 64 KiB of form fields. A 413 names the budget, the limit, and the asked size.

Postgres is not wired. The schema stays boring (text ids, integer millis) so that can be a URL swap later.

## Auth

Username and password. Argon2 hashes. Opaque bearer tokens. A session lasts until logout.

`POST /v1/auth/register` and `POST /v1/auth/login` take `{ "username": "...", "password": "..." }` and return `{ "token", "username" }`. Username is 3 to 32 characters, letters, digits, underscore. Unique case-insensitively. Password is at least 8 characters.

All remaining API routes need `Authorization: Bearer <token>`. Static web files are public.

## Routes

Prefix `/v1`.

- `POST /auth/register`
- `POST /auth/login`
- `POST /auth/logout`
- `GET /me`
- `GET /library` → `{ "books": [ BookRecord ] }`
- `POST /books` multipart: required `epub`; optional `cover`, `title`, `author`, `handle`, `isSample`
- `GET /books/{id}`
- `DELETE /books/{id}`
- `GET /books/{id}/epub`
- `GET /books/{id}/posts` → `{ "posts": [ { "id": "0", "kind": "Heading" | "Body", "text": "...", "chapter": "..." } ] }`
- `GET /books/{id}/cover`
- `PATCH /books/{id}` `{ "progressIndex"? }` (`postCount` is accepted for old clients but ignored; the server owns it)
- `GET /books/{id}/likes` `{ "ids": [...] }`
- `POST /books/{id}/likes/{postId}` toggle
- `GET /books/{id}/comments` `{ "counts": { "0": 2 }, "mine": ["0"] }`
- `GET /books/{id}/comments/{postId}` `{ "comments": [ Comment ] }`
- `POST /books/{id}/comments/{postId}` `{ "text": "..." }`
- `DELETE /books/{id}/comments/{postId}/{commentId}`
- `GET /profiles/{username}/replies` `{ "username", "replies": [ { "bookId", "postId", "comment" } ] }` newest first. Unknown username is an empty list.

Book JSON matches the phone's `BookRecord`: `id`, `title`, `author`, `handle`, `importedAt`, `postCount`, `progressIndex`, `isSample`, `mine`.

Every signed-in account can read every book. `mine` is true when you added it. Only that account can delete it. Reading place is per account.

Uploads are parsed before entering the shelf. Missing or empty title/author fields use OPF metadata, then `Untitled` / `Unknown`. Missing handles use the phone's slug rule: lowercase ASCII letters/digits, first 18, or `author` when empty. A missing cover uses the EPUB's cover image if it fits the existing 4 MiB cover budget. Covers keep the `cover.jpg` filename on disk, but the response identifies JPEG, PNG, GIF, or WebP from magic bytes. Bad EPUBs return 400 `Could not read that EPUB: <reason>`.

Posts are stored in each book's `posts.json`; IDs are sequential decimal index strings. Books uploaded before server parsing get their feed and `postCount` filled lazily on the first `GET /books/{id}/posts`. Concurrent fills are serialized and caches use a temporary file plus atomic rename.

Feed assembly ports Android's TOC title selection, publisher-chrome skipping, heading de-duplication and chapter tracking. HTML segmentation follows Readium Kotlin 3.3.0's block/media/line-break boundaries, using the block tag table from JSoup 1.22.2 (the version Readium 3.3.0 ships), so `ins`, `del`, `canvas`, `audio`, and `video` stay inline and whitespace rules (including non-breaking spaces). See `src/epub.rs` for deviations: HTML5 tree repair, one entity-decoding pass, UTF-8 input, skipping noscript/template fallback text, and explicit h1–h6 heading roles (Readium currently emits Body roles). Heading recognition changes kinds/chapters, not indexes; parser differences in unusual/malformed documents can affect legacy phone indexes. The parser does not render CSS or support DRM.

`isSample=true` keeps one sample for the server. A second upload returns the first row.

A comment is at most 2000 characters. The compose field shows about 8 lines of 40 characters. A long paragraph in the sample books stays under 1000. 2000 is six times the visible field. A 400 names the budget and the asked length. Empty text is 400 "Write something first."

## Tests

```bash
cargo test
```

Register, login, real EPUB uploads and metadata defaults, canonical posts, lazy cache fills, bad EPUBs, cover MIME detection, static/history routing, progress, likes, comments, profile replies, download, delete, shared books, per-person progress, signup off, oversized EPUB. Parser unit tests cover EPUB3 nav, EPUB2 NCX, a broken NCX, front matter, heading de-duplication, entities/segmentation, JSoup inline tags, the text budget, and the bundled sample.
