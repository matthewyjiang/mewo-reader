# Server

Hosted library for Mewo, and the process that serves the web app. Phones call it through `HostedLibraryStore`. Local phone books stay on the device and still use Readium.

A book is private to the account that added it until that account shares it. The built-in sample is visible to every signed-in account. Likes and reading place stay per account. Notes on a line are visible to accounts that can see the book. `GET /library` returns books you added, books other accounts shared, and the sample.

Rust 1.88 (`rust-toolchain.toml`). Version strings: [root README](../README.md#version-and-release).

## Run

```bash
cargo run
```

Listens on `0.0.0.0:8787`. Data lands in `./data` (`mewo.db` plus uploaded files). On a phone on the same LAN the URL is `http://<that-machine>:8787`.

`GET /v1/version` is public and reports the version compiled into this binary.

## Docker

From the repository root. The image builds `server/` and `web/`.

```bash
docker build -f server/Dockerfile -t mewo-server .
docker run --rm -p 8787:8787 -v mewo-data:/data mewo-server
```

`.dockerignore` excludes `.git`, so a plain `docker build` has nothing for `git describe` to read. Pass the release identity as build args, or the image uses the `Cargo.toml` / `web/package.json` version and a null commit. `MEWO_VERSION` is the tag with the leading `v` removed (`v0.1.5` → `0.1.5`). `MEWO_COMMIT` is the 7-character sha. Both are compile-time. Setting them on `docker run` does not change `GET /v1/version`.

```bash
docker build -f server/Dockerfile \
  --build-arg MEWO_VERSION=0.1.5 \
  --build-arg MEWO_COMMIT=abc1234 \
  -t mewo-server .
```

The image serves the web build at `/` with `MEWO_WEB_DIR=/srv/web`. Keep `/data` on a volume. The process runs as uid 10001. A bind-mounted host directory has to be writable by that uid.

Pushes to `main` that touch `server/`, `web/`, `.dockerignore`, or `.github/workflows/server-image.yml` publish `ghcr.io/matthewyjiang/mewo-reader/server`, tagged `latest` and `sha-<commit>`. Those builds pass the sha and an empty version, so `/v1/version` reports the crate version plus that sha. A `v*` tag publishes the image tagged with that version and bakes the tag and sha into the binary and the web bundle. `workflow_dispatch` publishes too. The same tag builds the Android APK. Deploying the image is outside this repo.

```bash
docker pull ghcr.io/matthewyjiang/mewo-reader/server:latest
docker run --rm -p 8787:8787 -v mewo-data:/data ghcr.io/matthewyjiang/mewo-reader/server:latest
```

The package is private until it is made public. A 403 on pull means the token cannot read packages, or the package is still private.

## Env

Runtime:

- `MEWO_LISTEN` — default `0.0.0.0:8787`
- `MEWO_DATA_DIR` — default `data`
- `MEWO_WEB_DIR` — unset means API only. An existing directory is served as static files, with `index.html` for other non-API paths. Relative paths are relative to the process working directory. Unknown `/v1/...` routes stay a JSON 404. From `server/`, a local web build is `MEWO_WEB_DIR=../web/dist`. The image sets `/srv/web`
- `MEWO_DATABASE_URL` — default `sqlite://<data>/mewo.db?mode=rwc`. SQLite only
- `MEWO_ALLOW_SIGNUP` — default on when unset. A set value is on only for `1`, `true`, `yes`, or `on` (any case). Use `false` when the server faces the internet
- `MEWO_MAX_EPUB_BYTES` — default `67108864` (64 MiB)

Compile-time, via `build.rs`: `MEWO_VERSION`, `MEWO_COMMIT`. See [Version and release](../README.md#version-and-release).

Parsing reads at most `MAX_EPUB_TEXT_BYTES` (32 MiB) of inflated XML/XHTML per book (`src/epub.rs`). A cover is at most 4 MiB. The HTTP body limit is the EPUB cap plus 4 MiB plus 64 KiB of form fields. Over budget is a 400 or 413 that names the budget, the limit, and the asked size.

## Auth

Username and password. Argon2 hashes. Opaque bearer tokens. A session lasts until logout.

`POST /v1/auth/register` and `POST /v1/auth/login` take `{ "username", "password" }` and return `{ "token", "username" }`. Username is 3 to 32 characters, letters, digits, underscore, unique case-insensitively. Password is 8 to 256 characters.

`GET /v1/version` is public. Static web files are public. Every other `/v1` route needs `Authorization: Bearer <token>`.

## Routes

Prefix `/v1`.

- `GET /version` → `{ "version", "commit" }`. `commit` is a string or null. Public
- `POST /auth/register`
- `POST /auth/login`
- `POST /auth/logout` → 204
- `GET /me` → `{ "username" }`
- `GET /search?q=&type=accounts|books|posts&cursor=` → a page of 20. `q` is trimmed; empty is 400 `Enter a search.` More than 100 characters is 400 and names the limit and the asked length. `cursor` is the previous `nextCursor`. A bad cursor is 400
- `GET /users/{username}` → `ProfileRecord`. Unknown username is 404. A username that fails the register rules is 400
- `GET /users/{username}/posts?cursor=` → `ProfilePostsPage`, 30 replies, newest first. Unknown username is 404
- `GET /users/{username}/reposts?cursor=` → `ProfileRepostsPage`, 30 reposts, newest first. Unknown username is 404
- `GET /users/{username}/books` → `{ "items": [ BookRecord ] }`. Books that account added with `shared` true, excluding the sample. A private book is omitted on your own profile too. Unknown username is 404
- `GET /feed?cursor=` → `FeedPage`, 20 rows, newest first. Comments and reposts on books you can see, including your own
- `GET /library` → `{ "books": [ BookRecord ] }`
- `POST /books` multipart. Required `epub`. Optional `cover`, `title`, `author`, `handle`, `isSample`
- `GET /books/{id}`
- `PATCH /books/{id}` `{ "progressIndex"?, "shared"? }`. `postCount` is accepted and ignored. Only the owner may set `shared`. Another account that can see the book gets 403. A book you cannot see is 404
- `DELETE /books/{id}` → 204. Owner only. Anyone else who can see it gets 403
- `GET /books/{id}/epub`
- `GET /books/{id}/posts` → `{ "posts": [ { "id": "0", "kind": "Heading" | "Body", "text", "chapter" } ] }`
- `GET /books/{id}/cover`
- `GET /books/{id}/likes` → `{ "ids" }`
- `POST /books/{id}/likes/{postId}` toggles
- `PUT /books/{id}/posts/{postId}/repost` → `{ "reposts": 1, "reposted": true }`. A second PUT keeps the count and the original time
- `DELETE /books/{id}/posts/{postId}/repost` → the same shape. A second DELETE keeps `{ "reposts": 0, "reposted": false }` once yours is gone
- `GET /books/{id}/comments` → `{ "counts": { "0": 2 }, "mine": ["0"], "reposts": { "0": 1 }, "reposted": ["0"] }`
- `GET /books/{id}/comments/{postId}` → `{ "comments": [ { "id", "username", "text", "createdAt", "mine" } ] }`
- `POST /books/{id}/comments/{postId}` `{ "text" }`
- `DELETE /books/{id}/comments/{postId}/{commentId}`
- `GET /profiles/{username}/replies` → `{ "username", "replies": [ { "bookId", "postId", "comment" } ] }`, newest first. An unknown username is an empty list. Lines from a book you cannot see are omitted. Shipped clients call this. `GET /users/{username}` is the newer profile and 404s an unknown username

Book JSON: `id`, `title`, `author`, `handle`, `importedAt`, `postCount`, `progressIndex`, `isSample`, `mine`, `shared`, `ownerUsername`.

Search JSON is one object. `type` is `accounts`, `books`, or `posts`. `nextCursor` is a string or null.

`{ "type": "accounts", "items": [{ "username": "ada", "mine": false }], "nextCursor": null }`

`{ "type": "books", "items": [BookRecord], "nextCursor": null }`

`{ "type": "posts", "items": [{ "book": BookRecord, "post": { "id": "1", "kind": "Body", "text": "...", "chapter": "..." } }], "nextCursor": null }`

Accounts match `username` with a case-insensitive substring. A prefix ranks before an infix, then shorter names. Books match title, author, handle, or the owner's username. A title prefix ranks first, then newer `importedAt`. Posts match the line or its chapter. Rank is FTS5 bm25, then newer books, then the post index. Private books are missing from book and post hits, profile replies, profile reposts, and the home feed, unless you own them. Sharing or the sample makes them visible without a reindex. `postCount` is how many of that account's replies sit on books you can see. `repostCount` is the same for reposts.

`ProfileRecord` is `{ "username", "joinedAt", "postCount", "repostCount", "mine" }`. `joinedAt` is the account's `created_at` in milliseconds.

`ProfilePostsPage` is `{ "items": [{ "book": BookRecord, "post": Post, "comment": CommentRecord }], "nextCursor" }`. `comment` is `{ "id", "username", "text", "createdAt", "mine" }`. `nextCursor` is opaque. Pass it back as `cursor`.

`ProfileRepostsPage` is `{ "items": [{ "book": BookRecord, "post": Post, "repostedAt": 1710000000000 }], "nextCursor" }`. `repostedAt` is milliseconds.

`FeedPage` is `{ "items": [ FeedItem ], "nextCursor" }`. A `FeedItem` is one of:

`{ "type": "comment", "actor": { "username": "ada" }, "book": BookRecord, "post": Post, "comment": CommentRecord, "at": 1710000000000 }`

`{ "type": "repost", "actor": { "username": "ada" }, "book": BookRecord, "post": Post, "at": 1710000000000 }`

`at` is milliseconds. Newest `at` first, then `repost` before `comment` at the same time, then the activity id. A line of a book shows up here when someone comments on it or reposts it. `reposts` on `GET /books/{id}/comments` is the per-post count (posts with zero are left out, same as `counts`). `reposted` is the post ids you reposted, same shape as `mine`. A book you cannot see is 404 for repost, same as likes and comments. Deleting a book deletes its reposts.

Migration `005_search.sql` adds the `posts` table, the FTS5 index, `books.posts_indexed`, and the `reposts` table. Snapshot `/data` before deploying a build that includes it. Books already on disk are indexed in the background at startup.

A new upload is private (`shared` false). Books already on the shelf when sharing shipped stay shared. Opening, downloading, or commenting on a book you cannot see is 404. Anyone who can see a book can move their own reading place, like a post, and comment.

Uploads are parsed before they enter the shelf. Empty title or author uses OPF metadata, then `Untitled` / `Unknown`. An empty handle uses the phone slug rule: lowercase ASCII letters and digits, first 18, or `author`. A missing cover uses the EPUB cover when it fits the 4 MiB cover budget. The file on disk is named `cover.jpg`; the response content type comes from magic bytes (JPEG, PNG, GIF, or WebP). A bad EPUB is 400 `Could not read that EPUB: <reason>`.

Posts live in each book's `posts.json`, which is the body of `GET /books/{id}/posts`. The same feed is copied into the `posts` table for search. IDs are sequential decimal index strings. Upload writes the file and the rows together. Books uploaded before server parsing get their feed and `postCount` filled on the first `GET /books/{id}/posts`. A book from before migration 005 is indexed at startup from `posts.json` when that file exists, otherwise by parsing the EPUB. Feed assembly is in `src/epub.rs` and `src/feed.rs`.

`isSample=true` keeps one sample. A second sample upload returns the first row.

A comment is at most 2000 characters. Empty text is 400 `Write something first.` A 400 names the budget and the asked length.

## Tests

```bash
cargo test
```
