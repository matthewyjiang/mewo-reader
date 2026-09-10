# Server

Hosted library for Mewo. The phone talks to this through `HostedLibraryStore`.

Local and hosted are different shelves. This process is the hosted one. Each account has its own books. The phone still parses EPUB files with Readium.

## Run

Rust 1.88. From this directory:

```bash
cargo run
```

Listens on `0.0.0.0:8787`. Data lands in `./data` (`mewo.db` plus uploaded files).

On a phone on the same LAN, the server URL is `http://<that-machine>:8787`.

## Env

- `MEWO_LISTEN` default `0.0.0.0:8787`
- `MEWO_DATA_DIR` default `data`
- `MEWO_DATABASE_URL` default `sqlite://<data>/mewo.db?mode=rwc`
- `MEWO_ALLOW_SIGNUP` default on. Set `0` or `false` when the box faces the internet.
- `MEWO_MAX_EPUB_BYTES` default `67108864` (64 MiB)

The cap is 64 MiB because the bundled sample is 2_886 bytes, Gutenberg War and Peace (epub3.images) is 1_824_099, and illustrated Pride and Prejudice (epub3.images) is 24_835_578. That leaves room for a heavy illustrated novel and still rejects a dump. A 413 names the budget, the limit, and the asked size.

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

Book JSON matches the phone's `BookRecord`: `id`, `title`, `author`, `handle`, `importedAt`, `postCount`, `progressIndex`, `isSample`.

A book that is not yours is 404, same as missing.

`isSample=true` keeps one sample per user. A second upload returns the first row.

## Tests

```bash
cargo test
```

Register, login, upload, progress, likes, download, delete, isolation between users, signup off, oversized EPUB.
