# Product

<!-- impeccable:product-schema 1 -->

## Platform

adaptive

## Stack

delegated: Kotlin and Jetpack Compose on Android, SwiftUI on iPhone, Svelte for the hosted web app. Both phones use the matching Readium toolkit to parse local EPUBs. Rust (axum, sqlx, SQLite) for the hosted library. The server parses hosted books once on upload and serves the web app. Clients read those posts, so post ids line up for likes and notes.

## Users

People who sideload an APK or install from Xcode or TestFlight, and who may sign into a server they run. No catalog and no store listing.

## Product Purpose

Import an EPUB and read it as a vertical feed, one paragraph per post.

## Positioning

The book is a timeline. Each paragraph is a post. The chrome is Twitter or X, light or dark.

## Operating Context

Phone first, one-handed. Books come from files already on the device, or from a hosted shelf. Local asks for a handle on first launch, and after an update if none is saved. Name is optional. That handle is not a password. Hosted uses a server account.

## Capabilities and Constraints

- Set a local handle on first launch, and after an update if none is saved. Name is optional. Change both in Settings → Account.
- Import an EPUB from the system file picker, or open the bundled sample.
- Keep a library on the device, or sign into a hosted server. The two shelves do not merge or sync.
- Read the hosted shelf in a browser at the server's own address. The web app has no local shelf. See [web/README.md](web/README.md).
- Read as a vertical feed: one post per paragraph, including long ones.
- Start the feed at the first non-publisher table-of-contents entry.
- Remember reading place per book, per account on a host. A book left on the last post sits under Finished on Home, and returns to the timeline if you leave off earlier.
- Jump to a heading from the chapter line or the reader title.
- Hide the title bar and tab bar when scrolling down a feed, and show them when scrolling up.
- Like a passage. Likes stay per account.
- Leave a note on a line. Local notes stay on the phone. Hosted notes are visible to accounts that can see the book.
- Search books and posts from books already opened.
- See liked lines in one feed.
- Open a profile from the account drawer or a reply. The profile lists that person's replies, each with the original line.
- Pick Twitter or X, light or dark. Pick how big posts read. Bars and buttons stay on the system size.
- On a hosted shelf, a new book is private until the owner shares it. The built-in sample is on every signed-in shelf. Home groups My books, Shared with me, and Finished. Only the owner can delete a book or change sharing. Anyone who can see a book can read it, like a post, comment, and move their own reading place.
- Settings → About shows the app build and, when a host is signed in, `GET /v1/version` plus the server URL.
- No OPDS or DRM.

## Brand Commitments

- Name: Mewo
- The reading surface is a timeline: avatar, name, handle, body, action row, hairline dividers, vertical scroll. Visual rules: [DESIGN.md](DESIGN.md).

## Evidence on Hand

No personal library is in the repo. A short public-domain sample is bundled so the empty state can open a book. Do not invent reviews, user counts, or store claims.

## Product Principles

- The feed is the reader.
- Local reading does not need a network. Hosted is a shelf you opt into.
- One-handed phone use is the default.
- Someone who receives an APK or a TestFlight install should understand the app without a tour.
