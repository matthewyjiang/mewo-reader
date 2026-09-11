# Server

Hosted library for Mewo. The phone talks to this through `HostedLibraryStore`.

Local and hosted are different shelves. This process is the hosted one. Books are a shared shelf. Likes and reading place stay per account. Notes on a line are public. The phone still parses EPUB files with Readium.

## Run

Rust 1.88. From this directory:

```bash
cargo run
```

Listens on `0.0.0.0:8787`. Data lands in `./data` (`mewo.db` plus uploaded files).

On a phone on the same LAN, the server URL is `http://<that-machine>:8787`.

## Docker

From this directory:

```bash
docker build -t mewo-server .
docker run --rm -p 8787:8787 -v mewo-data:/data mewo-server
```

Same env vars as below. Keep `/data` on a volume or the next `docker run` starts with an empty shelf.

Pushes to `main` that touch `server/` publish `ghcr.io/matthewyjiang/mewo-reader/server`. Tags: `latest`, `sha-<commit>`, and a semver tag if you push `v*`.

```bash
docker pull ghcr.io/matthewyjiang/mewo-reader/server:latest
docker run --rm -p 8787:8787 -v mewo-data:/data ghcr.io/matthewyjiang/mewo-reader/server:latest
```

The package is private until you flip it public under the repo's Packages tab. A 403 on pull means log in with a token that can read packages, or make the package public.

The process runs as uid 10001. If you bind-mount a host directory, that directory needs to be writable by that uid.

## Env

- `MEWO_LISTEN` default `0.0.0.0:8787`
- `MEWO_DATA_DIR` default `data`
- `MEWO_DATABASE_URL` default `sqlite://<data>/mewo.db?mode=rwc`
- `MEWO_ALLOW_SIGNUP` default on. Set `0` or `false` when the box faces the internet.
- `MEWO_MAX_EPUB_BYTES` default `67108864` (64 MiB)

The cap is 64 MiB because the bundled sample is 2_886 bytes, Gutenberg War and Peace (epub3.images) is 1_824_099, and illustrated Pride and Prejudice (epub3.images) is 24_835_578. That leaves room for a heavy illustrated novel and still rejects a dump. The HTTP body limit is that cap plus 4 MiB for a cover and 64 KiB of form fields. A 413 names the budget, the limit, and the asked size.

Postgres is not wired. The schema stays boring (text ids, integer millis) so that can be a URL swap later.

## Auth

Username and password. Argon2 hashes. Opaque bearer tokens. A session lasts until logout.

`POST /v1/auth/register` and `POST /v1/auth/login` take `{ "username": "...", "password": "..." }` and return `{ "token", "username" }`. Username is 3 to 32 characters, letters, digits, underscore. Unique case-insensitively. Password is at least 8 characters.

Everything else needs `Authorization: Bearer <token>`.

## Routes

Prefix `/v1`.

- `POST /auth/register`
- `POST /auth/login`
- `POST /auth/logout`
- `GET /me`
- `GET /library` → `{ "books": [ BookRecord ] }`
- `POST /books` multipart: `epub`, optional `cover`, `title`, `author`, `handle`, `isSample`
- `GET /books/{id}`
- `DELETE /books/{id}`
- `GET /books/{id}/epub`
- `GET /books/{id}/cover`
- `PATCH /books/{id}` `{ "postCount"?, "progressIndex"? }`
- `GET /books/{id}/likes` `{ "ids": [...] }`
- `POST /books/{id}/likes/{postId}` toggle
- `GET /books/{id}/comments` `{ "counts": { "0": 2 }, "mine": ["0"] }`
- `GET /books/{id}/comments/{postId}` `{ "comments": [ Comment ] }`
- `POST /books/{id}/comments/{postId}` `{ "text": "..." }`
- `DELETE /books/{id}/comments/{postId}/{commentId}`
- `GET /profiles/{username}/replies` `{ "username", "replies": [ { "bookId", "postId", "comment" } ] }` newest first. Unknown username is an empty list.

Book JSON matches the phone's `BookRecord`: `id`, `title`, `author`, `handle`, `importedAt`, `postCount`, `progressIndex`, `isSample`, `mine`.

Every signed-in account can read every book. `mine` is true when you added it. Only that account can delete it. Reading place is per account.

`isSample=true` keeps one sample for the server. A second upload returns the first row.

A comment is at most 2000 characters. The compose field shows about 8 lines of 40 characters. A long paragraph in the sample books stays under 1000. 2000 is six times the visible field. A 400 names the budget and the asked length. Empty text is 400 "Write something first."

## Tests

```bash
cargo test
```

Register, login, upload, progress, likes, comments, profile replies, download, delete, shared books, per-person progress, signup off, oversized EPUB.
