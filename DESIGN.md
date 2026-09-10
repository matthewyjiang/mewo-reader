---
name: Mewo
description: An EPUB reader that presents a book as an X-style timeline
colors:
  x-black: "#000000"
  x-paper: "#FFFFFF"
  x-ink: "#E7E9EA"
  x-ink-light: "#0F1419"
  x-mute: "#71767B"
  x-mute-light: "#536471"
  x-line: "#2F3336"
  x-line-light: "#EFF3F4"
  x-hover: "#16181C"
  x-hover-light: "#F7F9F9"
  x-blue: "#1D9BF0"
  x-pink: "#F91880"
  x-green: "#00BA7C"
  twitter-dim: "#15202B"
  twitter-ink: "#14171A"
  twitter-mute: "#657786"
  twitter-line: "#E1E8ED"
  twitter-hover: "#F5F8FA"
  twitter-dim-ink: "#F7F9F9"
  twitter-dim-mute: "#8B98A5"
  twitter-dim-line: "#38444D"
  twitter-dim-hover: "#1E2732"
  twitter-blue: "#1DA1F2"
  twitter-like: "#E0245E"
  twitter-green: "#17BF63"
typography:
  display:
    fontFamily: "Atkinson Hyperlegible"
    fontSize: "22sp"
    fontWeight: 700
    lineHeight: 1.18
  body:
    fontFamily: "Atkinson Hyperlegible"
    fontSize: "17sp"
    fontWeight: 400
    lineHeight: 1.41
  meta:
    fontFamily: "Atkinson Hyperlegible"
    fontSize: "13sp"
    fontWeight: 400
    lineHeight: 1.23
rounded:
  avatar: "999px"
  fab: "999px"
  pill: "999px"
spacing:
  post-gutter: "16dp"
  avatar-gap: "12dp"
  bar: "56dp"
components:
  post:
    backgroundColor: "{colors.x-black}"
    textColor: "{colors.x-ink}"
    padding: "12dp 16dp"
  like-active:
    textColor: "{colors.x-pink}"
  fab:
    backgroundColor: "{colors.x-blue}"
    textColor: "{colors.x-paper}"
    rounded: "{rounded.fab}"
    size: "56dp"
---

# Design

## Overview

Mewo looks like a home timeline, not a typeset book. The user picks the chrome: Twitter or X, light or night. Layout stays the same. One paragraph is one post. Hairline rules, circular avatars, a four-across action row that we trimmed to quote, like, and share.

## Colors

Four display palettes. The user picks one from the account drawer. First launch follows the system night setting and lands on an X palette.

X dark is lights-out `#000000` with `#E7E9EA` ink, `#71767B` meta, `#2F3336` rules, `#1D9BF0` blue, `#F91880` likes. X light is white paper and `#0F1419` ink.

Twitter dim is `#15202B` navy with `#F7F9F9` ink, `#8B98A5` meta, `#38444D` rules, `#1DA1F2` blue, `#E0245E` likes. Twitter light is white paper, `#14171A` ink, `#657786` meta, `#E1E8ED` rules.

Do not use Dynamic Color. Wallpaper hues would break the timeline.

## Typography

Atkinson Hyperlegible for chrome and body. Display and names are 17-22sp bold. Body posts are 17sp on 24sp. Meta (handle, chapter) is 13sp mute. Follow the system font-size setting. Do not introduce a serif "book" face on the reader.

## Layout

Phone first, edge to edge. A 56dp top bar with a 0.6dp rule under it. The brand mark sits on the true center of the bar, overlaid, so side actions cannot shove it. Twitter themes use the bird (blue on light, ink on dim). X themes use the current X mark in ink. A dummy profile photo sits on the left of Home, Search, and Likes. Tap it, or swipe from the left edge, to open the account drawer. A 52dp tab bar sits on the bottom with Home, Search, and Likes. Tapping a tab switches that feed. Opening a book is the post view: back returns to the feed you came from. On Home, Likes, Search results, and the reader, the bars slide away when you scroll down and return when you scroll up. Search keeps its field. The status bar and gesture inset stay, so posts never sit under system chrome. TalkBack keeps the bars on. Feed rows are 16dp from the left, 40dp avatar, 12dp gap, then the post. Quote, like, and share sit across the post column. The add action is one circular FAB on the library, above the tab bar. System Back also leaves the reader.

## Elevation & Depth

No cards. No drop shadows. Separation is the hairline. The FAB is the only floating object.

## Shapes

Avatars and the FAB are circles. The empty-state add control is a pill. Posts have no radius because they are not containers.

## Components

**Top bar.** Home, Search, and Likes: dummy profile on the left, centered bird or X. Reader is the post view: back, book title. The title opens the chapter sheet. No elevation. Hairline under it. Home, Likes, and the reader hide this bar on scroll down and show it on scroll up. Search keeps its field and the profile button.

**Tab bar.** Home, Search, Likes. Filled icon for the current tab. Hairline above it. Hides with the top bar on scroll. Search keeps its field and still hides this bar.

**Search.** A live field in the top bar. Book hits, then post hits from books already opened.

**Likes.** One feed of every hearted line. Tap a line to jump to it in the book.

**Account drawer.** Left sheet, square corners, 300dp. Dummy profile at the top: letter avatar, name You, handle @local. No account. Hairline, then Settings. Display lives here: four swatches in a 2x2, Twitter light, X light, Twitter dim, Lights out. Selecting a swatch applies it and keeps the drawer open. Below them, **Mewo mode** is an accessible switch that uses the supplied Teddy artwork for the launcher icon, launch screen, and home header mark without changing the palette. Off by default; turning it off restores the open-book launcher/launch logo and the theme's bird or X header. The drawer scrolls on smaller screens. Edge swipe opens it on Home, Search, and Likes, not in the reader. The persisted Android launcher-component selection is the mode's source of truth, avoiding a separate preference that could disagree after backup restore. Launcher aliases point to short-lived themed entry activities, which hand off to the always-enabled reader activity. This keeps icon changes from dismissing the drawer and preserves the open book when returning from the launcher.

**Chapter sheet.** Bottom sheet. Headline is the book title. Rows are heading posts from the feed, hairline separated. The heading you are in uses hover fill and ink. Tap jumps to that post at once, with no scroll animation, and dismisses. Empty copy: "This book has no chapter headings." Open it from the reader title or `@handle · chapter`. No list icon. Publisher chrome (cover, copyright, contents, also-by) never reaches the feed, so it is not in this list. A prologue or author's note does.

**Post.** Author name bold, `@handle · chapter` mute on one line, then the paragraph. Heading posts use title weight. Like, quote, and share sit in 40dp targets. The chapter line opens the chapter sheet.

**Book row.** Same skeleton as a post. Title is the name. Progress is the body line.

**Empty timeline.** Headline, one sentence, pill to add a file, text button for the sample.

## Do's and Don'ts

- Do keep posts flush to the feed, not inside cards.
- Do let a long paragraph stay one post.
- Don't paginate.
- Don't add a serif reading theme or a cream paper background.
- Don't invent a second primary action on the library.
