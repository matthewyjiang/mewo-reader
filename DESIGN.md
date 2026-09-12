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

Mewo looks like a home timeline, not a typeset book. The user picks the chrome: Twitter or X, light or night. Layout stays the same. One paragraph is one post. Hairline rules, circular avatars, a four-across action row: comment, quote, like, and share.

## Colors

Four display palettes. The user picks one in Display, under Settings. First launch follows the system night setting and lands on an X palette.

X dark is lights-out `#000000` with `#E7E9EA` ink, `#71767B` meta, `#2F3336` rules, `#1D9BF0` blue, `#F91880` likes. X light is white paper and `#0F1419` ink.

Twitter dim is `#15202B` navy with `#F7F9F9` ink, `#8B98A5` meta, `#38444D` rules, `#1DA1F2` blue, `#E0245E` likes. Twitter light is white paper, `#14171A` ink, `#657786` meta, `#E1E8ED` rules.

Do not use Dynamic Color. Wallpaper hues would break the timeline.

## Typography

Atkinson Hyperlegible for chrome and body. Display and names are 17-22sp bold. Body posts default to 17sp on 24sp. Display has a seven-step font size control from 14sp to 28sp that scales post body and heading text only. Bars, names, handles, and buttons stay on the theme scale. Follow the system font-size setting as well; sizes stay in sp. Do not introduce a serif "book" face on the reader.

## Layout

Phone first, edge to edge. A 56dp top bar with a 0.6dp rule under it. The brand mark sits on the true center of the bar, overlaid, so side actions cannot shove it. Twitter themes use the bird (blue on light, ink on dim). X themes use the current X mark in ink. A dummy profile photo sits on the left of Home, Search, and Likes. Tap it, or swipe from the left edge, to open the account drawer. A 52dp tab bar sits on the bottom with Home, Search, and Likes. Tapping a tab switches that feed. Opening a book is the post view: back returns to the feed you came from. A profile is the same kind of destination: back returns to the feed you came from, and the tab bar stays. On Home, Likes, Search results, the reader, and a profile, the bars slide away when you scroll down and return when you scroll up. Search keeps its field. The status bar and gesture inset stay, so posts never sit under system chrome. TalkBack keeps the bars on. Feed rows are 16dp from the left, 40dp avatar, 12dp gap, then the post. Comment, quote, like, and share sit across the post column. The add action is one circular FAB on the library, above the tab bar. System Back also leaves the reader and a profile.

## Elevation & Depth

No cards. No drop shadows. Separation is the hairline. The FAB is the only floating object.

## Shapes

Avatars and the FAB are circles. The empty-state add control is a pill. Posts have no radius because they are not containers.

## Components

**Top bar.** Home, Search, and Likes: dummy profile on the left, centered bird or X. Reader is the post view: back, book title. The title opens the chapter sheet. No elevation. Hairline under it. Home, Likes, and the reader hide this bar on scroll down and show it on scroll up. Search keeps its field and the profile button.

**Tab bar.** Home, Search, Likes. Filled icon for the current tab. Hairline above it. Hides with the top bar on scroll. Search keeps its field and still hides this bar. When the keyboard is open, this bar sits above it.

**Search.** A live field in the top bar. Book hits, then post hits from books already opened.

**Likes.** One feed of every hearted line. Tap a line to jump to it in the book.

**Onboarding.** Full-screen gate on first launch and on an APK update when no handle is saved. Centered brand mark in the 56dp bar. Headline "This is you." One mute line: the drawer and your notes use this. A live preview of the letter avatar, name, and @handle, then Name (optional) and Handle fields. Bottom bar with a blue Next pill, dimmed until the handle is valid. Handle is 3 to 32 characters, letters, digits, underscore. Name max 50. System Back backgrounds the app. After Next, the library opens. This is not a tour and has no skip.

**Account drawer.** Left sheet, square corners, 300dp. Letter avatar at the top using the local name, or the hosted username once you sign into a server. Name and @handle follow the same rule. Tap the avatar, name, or handle to close the drawer and open that profile. Hairline, then one Settings row. Tap it to close the drawer and open Settings. The drawer does not hold display or library controls. A horizontal swipe from the left edge opens it on Home, Search, and Likes. The system Back button on Home sends the app to the background. The reader, Settings, and a profile keep Back for leaving those screens; that edge does not open the drawer.

**Settings.** Full-screen destination. Back and a centered title. The home page is three rows, Account, Display, and Library, each with a mute line and a chevron. Hairline between them. No controls on this page. Tap a row to open that nested page. Back from a nested page returns to Settings. Back from Settings returns to the feed you came from. The tab bar hides while Settings is open.

**Account.** Nested Settings page. Same live preview and fields as onboarding. A blue Save pill, dimmed until the handle is valid and something changed. This is the local name and handle. Hosted username stays under Library.

**Display.** Nested Settings page. Four swatches in a 2x2: Twitter light, X light, Twitter dim, Lights out. Selecting a swatch applies it and stays on the page. Below them, **Font size** is a stepped slider with Aa at each end and a live post preview from the sample. It changes how posts read and leaves chrome alone. Below that, **Mewo mode** is an accessible switch that uses the supplied Teddy artwork for the launcher icon, launch screen, and home header mark without changing the palette. Off by default; turning it off restores the open-book launcher/launch logo and the theme's bird or X header. On Android the persisted launcher-component selection is the mode's source of truth, avoiding a separate preference that could disagree after backup restore. Launcher aliases point to short-lived themed entry activities, which hand off to the always-enabled reader activity. This keeps icon changes from dismissing Settings and preserves the open book when returning from the launcher. On iPhone, UserDefaults is the source of truth and `setAlternateIconName` swaps the icon. VoiceOver keeps the bars on, same as TalkBack.

**Library.** Nested Settings page. Two choices: On this phone, or Hosted server. Local keeps EPUB files, likes, notes, and reading place on the device. Hosted is a shared shelf: every signed-in account sees the same books. Notes on a line are public. Likes and reading place stay per person. Only the person who added a book can remove it. Pick Hosted and the page shows a server URL, username, password, Sign in, and Create account. After sign-in the drawer handle is that username and Sign out is the way out. The shelves do not merge.

**Chapter sheet.** Bottom sheet. Headline is the book title. Rows are heading posts from the feed, hairline separated. The heading you are in uses hover fill and ink. Tap jumps to that post at once, with no scroll animation, and dismisses. Empty copy: "This book has no chapter headings." Open it from the reader title or `@handle · chapter`. No list icon. Publisher chrome (cover, copyright, contents, also-by) never reaches the feed, so it is not in this list. A prologue or author's note does.

**Post.** Author name bold, `@handle · chapter` mute on one line, then the paragraph. Heading posts use title weight. Comment, quote, like, and share sit in 40dp targets. Comment is leftmost. A mute count sits next to the bubble when notes exist. The bubble uses accent when you have a note on that line. The chapter line opens the chapter sheet.

**Comment sheet.** Full-height sheet that matches X's reply composer. Close on the left, a blue Reply pill on the right, dimmed until there is text. The line is a tweet. A 2dp rule runs down the avatar column through existing replies to your compose row. Replies are tweets: 40dp avatar, name, `@handle · time`, body. Tap a reply avatar, name, or handle to close the sheet and open that profile. Your replies have a ••• menu with Delete. Compose uses your avatar and "Post your reply". Focus or text shows "Replying to @handle" in accent. Open it from the comment action. No comments tab.

**Profile.** Full-screen destination. Back, the name, and "N posts" in the 56dp bar. A 125dp hover banner, then a 68dp letter avatar overlapping it with a 4dp ground ring. Name, @handle, and one Posts tab with an accent underline. The tab is replies for now: original line above, reply below, 2dp rule down the avatar column, action row on the line. Newest first. Empty copy: "Reply to a line. It shows up here." on your profile, "No posts yet." on someone else's. Tap a row to open that line's comment sheet. The tab bar stays. Back leaves.

**Book row.** Same skeleton as a post. Title is the name. Progress is the body line.

**Empty timeline.** Headline, one sentence, pill to add a file, text button for the sample.

## Do's and Don'ts

- Do keep posts flush to the feed, not inside cards.
- Do let a long paragraph stay one post.
- Don't paginate.
- Don't add a serif reading theme or a cream paper background.
- Don't invent a second primary action on the library.
