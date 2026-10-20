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
- `MEWO_ALLOW_SIGNUP` — first-boot seed only. Default on when unset. A set value is on only for `1`, `true`, `yes`, or `on` (any case). After the settings row exists, the database wins. A set variable that disagrees with the row logs one warning and is ignored
- `MEWO_MAX_EPUB_BYTES` — first-boot seed only. Default `67108864` (64 MiB). A seed outside 1 MiB to 1 GiB is clamped into that range, with one warning that names the variable. After the settings row exists, the database wins. A set variable that disagrees with the row logs one warning and is ignored. The panel accepts 1 to 1024 MB
- `MEWO_DOWNLOADS_DIR` — completed SABnzbd folders are read only from here. Unset means a request can be created but the worker will not import. The path is not a setting: `GET /v1/admin/settings` reports it as `downloadsDir` (or null), and `PATCH` rejects that key. A folder is imported only when its canonical path stays inside this directory and its name is `mewo-<request id>` or `mewo-<request id>.<n>`
- `MEWO_OPENLIBRARY_URL` — catalog search base. Default `https://openlibrary.org`. Empty is the same as unset
- `MEWO_TRUSTED_PROXY_HOPS` — how many reverse proxies append an address to `X-Forwarded-For`. Default `0`, which ignores that header and uses the socket peer. A value that does not parse is `0`, with one warning. Deployments behind a reverse proxy should set it to `1`. If the header has fewer addresses than the hop count, the leftmost address that is present is used. Per-IP login and register limits apply only to globally routable clients: loopback, RFC1918 private, link-local, CGNAT (`100.64.0.0/10`), and IPv6 ULA (`fc00::/7`) skip the IP bucket and are still limited per username

Compile-time, via `build.rs`: `MEWO_VERSION`, `MEWO_COMMIT`. See [Version and release](../README.md#version-and-release).

Parsing reads at most `MAX_EPUB_TEXT_BYTES` (32 MiB) of inflated XML/XHTML per book (`src/epub.rs`). A cover is at most 4 MiB. JSON and other non-upload routes reject a body over 1 MiB with 413. `POST /v1/books` accepts up to 1 GiB plus 4 MiB plus 64 KiB of form fields. The live EPUB cap is the settings value, checked while the upload streams. Over that cap is 413 `EPUB too large: asked N bytes, limit is N bytes`.

## Auth

Username and password. Argon2 hashes. Opaque bearer tokens. A session lasts until logout.

`POST /v1/auth/register` and `POST /v1/auth/login` take `{ "username", "password" }` and return `{ "token", "username" }`. Username is 3 to 32 characters, letters, digits, underscore, unique case-insensitively. Password is 8 to 256 characters.

`GET /v1/version` and `GET /v1/instance` are public. Static web files are public. Every other `/v1` route needs `Authorization: Bearer <token>`, except `POST /v1/auth/register` and `POST /v1/auth/login`.

## Admin

`users.is_admin` is a boolean. Existing accounts stay non-admins. The CLI grants the first one. The control panel is web-only.

With no arguments, `mewo-server` serves, same as today. Admin commands are the same binary and the same database (`MEWO_DATA_DIR`, default `./data`):

```bash
mewo-server admin list
mewo-server admin grant <username>
mewo-server admin revoke <username>
mewo-server admin create <username>
```

`grant` and `revoke` are idempotent and exit 0. `grant` prints `<username> is an admin.` `revoke` prints `<username> is not an admin.` An unknown account exits 1 with `No account named <username>.` A bad command prints usage on stderr and exits 2. A database error exits 2.

`list` prints one account per line, ordered by `created_at`. An admin line is the name padded to 8 columns, a space, then `admin`:

```text
matt     admin
ada
```

`create` inserts an admin even when signups are off. The password comes from stdin, never from argv or the environment. On a terminal it prompts with no echo. A pipe reads one line.

```bash
kubectl -n mewo exec deploy/mewo-server -- mewo-server admin list
kubectl -n mewo exec deploy/mewo-server -- mewo-server admin grant matt
kubectl -n mewo exec -it deploy/mewo-server -- mewo-server admin create matt
```

Migration `006_admin.sql` adds `users.is_admin` (default 0) and the `settings` table. Snapshot `/data` before deploying it. The previous binary ignores the extra column and table, but sqlx will not start on a database whose migration it does not know, so a rollback restores that snapshot.

Migration `007_requests.sql` adds `book_requests` and `settings.secrets` (default `{}`). Snapshot `/data` before deploying it, same as 006.

## Requests

Settings > Server has a Book requests group. It stays off until an admin sets the Prowlarr URL, the SABnzbd URL, both API keys, and turns Requests on. `MEWO_DOWNLOADS_DIR` has to be set or completed jobs are not imported. Catalog search then shows on Search, and Settings > Requests lists the signed-in account's requests.

A request searches Prowlarr for usenet releases in ebook category 7020 (`indexerIds=-1`, 90 second timeout). Other upstream calls keep a 20 second timeout. Prowlarr's grab endpoint is never called. The chosen NZB URL is sent to SABnzbd `addurl` with `nzbname=mewo-<request id>` and the configured category (`books` by default). When that job completes, the folder under `MEWO_DOWNLOADS_DIR` named `mewo-<id>` or `mewo-<id>.<n>` is imported as a private book (`shared` false) whose id is the request id. mewo then deletes that folder. SABnzbd history delete is sent with `archive=0`; `del_files=1` does not remove a completed download. The catalog title and author win over the EPUB's own metadata. Catalog search calls Open Library at most once a second.

Each account can have 5 active requests (`requested` or `downloading`), and one active request per Open Library work. No release for 14 days, or three failed downloads, marks the request failed. Retry is for failed or canceled requests. Cancel removes the SABnzbd job when there is one.

API keys live in `settings.secrets`, not in the settings document. `GET` never returns them. `PATCH` accepts `prowlarrApiKey` and `sabnzbdApiKey` as a string of at most 256 characters, or `null` to clear.

## Routes

Prefix `/v1`.

- `GET /version` → `{ "version", "commit" }`. `commit` is a string or null. Public
- `GET /instance` → `{ "allowSignup" }`. Public. Read live from the settings row. A future `name` is one more field
- `POST /auth/register`
- `POST /auth/login`
- `POST /auth/logout` → 204
- `GET /me` → `{ "username", "isAdmin", "canRequest" }`. `canRequest` is true when book requests are enabled
- `GET /admin/settings` → `{ "allowSignup", "maxEpubBytes", "requestsEnabled", "prowlarrUrl", "sabnzbdUrl", "sabnzbdCategory", "downloadsDir", "secrets": { "prowlarrApiKey", "sabnzbdApiKey" } }`. The secret fields are booleans (set or not). `downloadsDir` is the `MEWO_DOWNLOADS_DIR` path, or null. Admin
- `PATCH /admin/settings` JSON object of keys to change. Returns the full settings. An empty object is 200 and changes nothing. An unknown key, a wrong type, or an out-of-range value is 400 and names the key. `maxEpubBytes` must be 1 to 1024 MB (`maxEpubBytes must be 1 to 1024 MB.`). `prowlarrApiKey` and `sabnzbdApiKey` are a string or `null` and are not echoed back. `downloadsDir` is rejected
- `GET /catalog/search?q=&page=` → `{ "items": [ { "workKey", "title", "author", "year", "coverId", "request" } ], "nextPage" }`. `request` is a book request or null. Empty `q` is 400 `Enter a search.` Page size is 20. 403 `Requests are off.` when requests are disabled. An Open Library failure is 502
- `POST /requests` `{ "workKey", "title", "author", "year", "coverId" }` → the book request, 201, or 200 when this account already has an active request for that work. 409 `You have 5 requests in progress.` `workKey` must start with `/works/`. 403 when requests are off
- `GET /requests?all=` → `{ "items": [ BookRequest ] }`, newest first. `all=1` is admins only, otherwise 403 `Admins only.` 403 when requests are off
- `DELETE /requests/{id}` → 204. Owner or admin. Anyone else is 404. An imported request is 409 `That book is on your shelf.`
- `POST /requests/{id}/retry` → the book request. Failed or canceled only; otherwise 409 `Only a failed or canceled request can be retried.`
- `POST /admin/requests/test` → `{ "checks": [ { "name": "prowlarr" | "sabnzbd" | "downloads", "ok", "detail" } ] }`. 200 even when a check fails. Admin, including while requests are off

A book request is `{ "id", "workKey", "title", "author", "year", "coverId", "state", "progress", "error", "bookId", "requester", "createdAt", "updatedAt" }`. `state` is `requested`, `downloading`, `imported`, `failed`, or `canceled`.

- `GET /admin/users` → `{ "items": [ { "username", "joinedAt", "isAdmin", "books", "mine" } ] }`. Ordered by `created_at`. `mine` is true only on the caller. Admin
- `PATCH /admin/users/{username}` `{ "isAdmin" }` → that row. Unknown username is 404. Your own username is 409 `You can't change your own admin access.` A body that is not a JSON object (wrong type, unknown field, or non-JSON) is 400 `User patch must be a JSON object.` Idempotent. Admin
- `GET /admin/server` → `{ "users", "books", "comments", "reposts", "storageBytes" }`. `storageBytes` is `mewo.db`, `mewo.db-wal`, and `books/`. Admin

Every `/admin` route is 401 without a token and 403 `Admins only.` for a signed-in account that is not an admin. A revoke applies on the next request.
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
- `PUT /books/{id}/posts/{postId}/repost` → `{ "reposts": 1, "reposted": true }`. A second PUT keeps the count and the original time. `postId` is the canonical decimal index (`0`, `1`, …). `00`, `+0`, and `abc` are 400. A post that is not in the feed is 404
- `DELETE /books/{id}/posts/{postId}/repost` → the same shape when you can see the book, including a second DELETE after yours is already gone. After the book is unshared, DELETE still removes your own row and returns `{ "reposts": 0, "reposted": false }` without anyone else's count. A DELETE of a book you cannot see, when you have no row, is 404
- `GET /books/{id}/comments` → `{ "counts": { "0": 2 }, "mine": ["0"], "reposts": { "0": 1 }, "reposted": ["0"] }`
- `GET /books/{id}/comments/{postId}` → `{ "comments": [ { "id", "username", "text", "createdAt", "mine" } ] }`
- `POST /books/{id}/comments/{postId}` `{ "text" }`. The post id follows the same canonical-index rule as repost, and a missing post is 404
- `DELETE /books/{id}/comments/{postId}/{commentId}`
- `GET /profiles/{username}/replies` → `{ "username", "replies": [ { "bookId", "postId", "comment" } ] }`, newest first. An unknown username is an empty list. Lines from a book you cannot see are omitted. Shipped clients call this. `GET /users/{username}` is the newer profile and 404s an unknown username

Book JSON: `id`, `title`, `author`, `handle`, `importedAt`, `postCount`, `progressIndex`, `isSample`, `mine`, `shared`, `ownerUsername`.

Search JSON is one object. `type` is `accounts`, `books`, or `posts`. `nextCursor` is a string or null.

`{ "type": "accounts", "items": [{ "username": "ada", "mine": false }], "nextCursor": null }`

`{ "type": "books", "items": [BookRecord], "nextCursor": null }`

`{ "type": "posts", "items": [{ "book": BookRecord, "post": { "id": "1", "kind": "Body", "text": "...", "chapter": "..." } }], "nextCursor": null }`

Accounts match `username` with a case-insensitive substring. A prefix ranks before an infix, then shorter names. Books match title, author, handle, or the owner's username. A title prefix ranks first, then newer `importedAt`. Posts match the line or its chapter. Rank is FTS5 bm25, then newer books, then the post index. `nextCursor` is an opaque keyset for that order. An offset is not a valid cursor. Private books are missing from book and post hits, profile replies, profile reposts, and the home feed, unless you own them. Sharing or the sample makes them visible without a reindex. `postCount` is how many of that account's replies sit on books you can see. `repostCount` is the same for reposts.

`ProfileRecord` is `{ "username", "joinedAt", "postCount", "repostCount", "mine" }`. `joinedAt` is the account's `created_at` in milliseconds.

`ProfilePostsPage` is `{ "items": [{ "book": BookRecord, "post": Post, "comment": CommentRecord }], "nextCursor" }`. `comment` is `{ "id", "username", "text", "createdAt", "mine" }`. `nextCursor` is opaque. Pass it back as `cursor`.

`ProfileRepostsPage` is `{ "items": [{ "book": BookRecord, "post": Post, "repostedAt": 1710000000000 }], "nextCursor" }`. `repostedAt` is milliseconds.

`FeedPage` is `{ "items": [ FeedItem ], "nextCursor" }`. A `FeedItem` is one of:

`{ "type": "comment", "actor": { "username": "ada" }, "book": BookRecord, "post": Post, "comment": CommentRecord, "at": 1710000000000 }`

`{ "type": "repost", "actor": { "username": "ada" }, "book": BookRecord, "post": Post, "at": 1710000000000 }`

`at` is milliseconds. Newest `at` first, then `repost` before `comment` at the same time, then the activity id. A line of a book shows up here when someone comments on it or reposts it. `reposts` on `GET /books/{id}/comments` is the per-post count (posts with zero are left out, same as `counts`). `reposted` is the post ids you reposted, same shape as `mine`. A book you cannot see is 404 for repost, same as likes and comments. Deleting a book deletes its reposts.

Migration `005_search.sql` adds the `posts` table (`id INTEGER PRIMARY KEY`, FTS `content_rowid` is that id), the FTS5 index, `books.posts_indexed`, `comments (created_at)`, and the `reposts` table. Snapshot `/data` before deploying a build that includes it. Books already on disk are indexed in the background at startup, one book at a time, with a short pause between books. If the write lock is busy, that book is tried again instead of being left unindexed. An EPUB that cannot be read, and has no `posts.json`, is marked `posts_indexed = -1` and is not parsed again. Comments already stored under a non-canonical post id are left in place; count them with `SELECT COUNT(*) FROM comments WHERE post_id GLOB '*[^0-9]*' OR (post_id GLOB '0*' AND post_id <> '0')` before release. They no longer show up as a different post.

A new upload is private (`shared` false). Books already on the shelf when sharing shipped stay shared. Opening, downloading, or commenting on a book you cannot see is 404. Anyone who can see a book can move their own reading place, like a post, and comment.

Uploads are parsed before they enter the shelf. Empty title or author uses OPF metadata, then `Untitled` / `Unknown`. An empty handle uses the phone slug rule: lowercase ASCII letters and digits, first 18, or `author`. A missing cover uses the EPUB cover when it fits the 4 MiB cover budget. The file on disk is named `cover.jpg`; the response content type comes from magic bytes (JPEG, PNG, GIF, or WebP). A bad EPUB is 400 `Could not read that EPUB: <reason>`.

Posts live in each book's `posts.json`, which is the body of `GET /books/{id}/posts`. The same feed is copied into the `posts` table for search. IDs are sequential decimal index strings. Upload writes the file and the rows together. Books uploaded before server parsing get their feed and `postCount` filled on the first `GET /books/{id}/posts`. A book from before migration 005 is indexed at startup from `posts.json` when that file exists, otherwise by parsing the EPUB. Feed assembly is in `src/epub.rs` and `src/feed.rs`.

`isSample=true` keeps one sample. A second sample upload returns the first row.

A comment is at most 2000 characters. Empty text is 400 `Write something first.` A 400 names the budget and the asked length.

## Tests

```bash
cargo test
```
