# Product

<!-- impeccable:product-schema 1 -->

## Platform

android

## Stack

delegated: Kotlin, Jetpack Compose, Readium Kotlin toolkit 3.x. Chosen because EPUB rendering is a native Android problem and Readium already parses publications into text elements we can feed.

## Users

Matt and a few people he might hand an APK to. They sideload EPUB files from the device and read them.

## Product Purpose

Mewo is an Android EPUB reader. You import a book from the file picker and read it as a vertical feed, one paragraph per post. Success is opening a real EPUB and scrolling it like a timeline without fighting pagination.

## Positioning

The book is a timeline. Each paragraph is a post from the author. Neighboring readers paginate or scroll a typeset page; this one does not.

## Operating Context

Used on a phone, often in bed or on a couch, one-handed. Books come from files the user already has. No account. No catalog. Share the APK, not a store listing.

## Capabilities and Constraints

- Import EPUB from the system file picker
- Persist the library on device
- Read as a vertical feed: one post per paragraph, including long ones
- Start the feed at the first non-publisher TOC entry, not copyright or contents
- Remember reading place per book
- Jump to a heading from the chapter line or the reader title
- Like a passage and keep that like
- Search books and opened posts
- See liked lines in one feed
- Pick Twitter or X display, light or dark
- No accounts, sync, OPDS, DRM, or iOS in this version

## Brand Commitments

- Name: Mewo
- Reading surface is pinned: it should look and feel like scrolling a timeline. Avatar, name, handle, body, action row, hairline dividers, vertical infinite scroll. Not a typeset book page. Chrome is Twitter or X, light or dark, chosen by the user.

## Evidence on Hand

No real library, covers, or user books in the repo. Bundle a short public-domain sample so the empty state can open something immediately. Do not invent reviews, user counts, or store claims.

## Product Principles

- The feed is the reader. If it looks like a book layout, it failed.
- Local files only. Nothing should require a network to read.
- One-handed phone use is the default scene.
- Friends getting an APK should understand the app without a tour.
