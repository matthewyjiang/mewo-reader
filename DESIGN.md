---
name: Mewo
description: An EPUB reader that presents a book as an X-style timeline
colors:
  black: "#000000"
  paper: "#FFFFFF"
  ink: "#E7E9EA"
  ink-light: "#0F1419"
  mute: "#71767B"
  mute-light: "#536471"
  line: "#2F3336"
  line-light: "#EFF3F4"
  hover: "#16181C"
  hover-light: "#F7F9F9"
  blue: "#1D9BF0"
  pink: "#F91880"
  green: "#00BA7C"
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
    backgroundColor: "{colors.black}"
    textColor: "{colors.ink}"
    padding: "12dp 16dp"
  like-active:
    textColor: "{colors.pink}"
  fab:
    backgroundColor: "{colors.blue}"
    textColor: "{colors.paper}"
    rounded: "{rounded.fab}"
    size: "56dp"
---

# Design

## Overview

Mewo looks like X's home timeline, not a typeset book. Dark is lights-out black. Light is X's white paper. One paragraph is one post. Hairline rules, circular avatars, a four-across action row that we trimmed to quote, like, and share.

## Colors

Night reading is the default scene. Dark uses `#000000` ground and `#E7E9EA` ink. Meta text is `#71767B`. Rules are `#2F3336`. Actions: blue for add and links, pink for a liked heart, green reserved for a quoted repost. Light swaps to white paper and `#0F1419` ink. Do not use Dynamic Color. Wallpaper hues would break the timeline.

## Typography

Atkinson Hyperlegible for chrome and body. Display and names are 17-22sp bold. Body posts are 17sp on 24sp. Meta (handle, chapter) is 13sp mute. Follow the system font-size setting. Do not introduce a serif "book" face on the reader.

## Layout

Phone first, edge to edge. A 56dp top bar with a 0.6dp rule under it. Feed rows are 16dp from the left, 40dp avatar, 12dp gap, then the post. The add action is one circular FAB on the library. System Back leaves the reader.

## Elevation & Depth

No cards. No drop shadows. Separation is the hairline. The FAB is the only floating object.

## Shapes

Avatars and the FAB are circles. The empty-state add control is a pill. Posts have no radius because they are not containers.

## Components

**Top bar.** Product name or book title. No elevation. Hairline under it.

**Post.** Author name bold, `@handle · chapter` mute on one line, then the paragraph. Heading posts use title weight. Like, quote, and share sit in 40dp targets.

**Book row.** Same skeleton as a post. Title is the name. Progress is the body line.

**Empty timeline.** Headline, one sentence, pill to add a file, text button for the sample.

## Do's and Don'ts

- Do keep posts flush to the feed, not inside cards.
- Do let a long paragraph stay one post.
- Don't paginate.
- Don't add a serif reading theme or a cream paper background.
- Don't invent a second primary action on the library.
