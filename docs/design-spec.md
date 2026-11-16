# Better Audible: design spec (source of truth for the UI build)

Designs live in Wonder: file "Better Audible"
https://app.wonder.so/harikrishnan-v-k/files/01a1dedd-389c-7230-b800-a5c182706c96
(fileId 01a1dedd-389c-7230-b800-a5c182706c96, pageId 01a1dedd-389d-7965-9467-eb6cbe543487).
Artboards: Library, In progress, Explore, Now Playing, Mini player (floating window + menu bar popover), Appearance settings.
If the Wonder MCP tools are available (load via ToolSearch), read artboards with get_element_code for exact values; otherwise use this file.

## Product
Native macOS Audible client in Rust + GPUI. Sign in to the user's own Audible account, browse the library, resume across devices
(position sync), download and play offline, explore the catalog. Premium, calm, native-Mac feel.

## Fonts (all open source, bundle in the app)
- UI: Allo by Josh Puckett (Regular 400, Medium 500, SemiBold 600; OFL, https://allo.joshpuckett.me)
- Book titles and the "time left" sentence: Literata (serif)
- Time readouts (big time-left numeral, percentages, menu bar time): Geist Pixel

## Icons
Hugeicons free set (MIT, `@hugeicons/core-free-icons`), stroke-rounded style at the set's 1.5 stroke, bundled as SVG in
`assets/icons` and addressed through the typed `ui::Icon` enum. This overrides the Lucide icons drawn in the Wonder mock-up;
use the nearest Hugeicons glyph for each (search, compass, library, clock, download, bookmark, user, layers, checkmark circle,
playlist, play, pause, go back/forward 30 s, moon, volume, airplay, arrow left, more, coins, maximize, headphones, wifi, battery, sort).

## Colour tokens
Light window (default):
- window / sidebar surface: #FFFFFF (sidebar is plain white, separated by a 1px hairline)
- ink: #241E2B (plum-black), ink-secondary: #6B6475
- hairline: rgba(36,30,43,0.10), fill (subtle control bg): rgba(36,30,43,0.06)
- accent (Ember): #D63F25, accent-top (gradient top): #EC5A3C, accent-deep (text on tint): #9E2A14,
  accent-tint (selected row bg): #FCEBE6, accent-glow: #FF8A6B
- button-top #FFFFFF, button-bottom #F3F0F5, ink-top #443B4F (dark play button gradient top)
Now Playing / mini player take their colour from the current cover (default sample: field #232A44, field-deep #161B2E,
on-field #F3EEF2, on-field-muted rgba(243,238,242,0.68), on-field-track rgba(243,238,242,0.18), light-button bottom #DCD5E3).
Derive field colours from the cover image (dominant colour, darkened); keep text contrast >= 4.5:1.
Other accent themes (each: accent, deep, tint): Fjord #0E7C7B/#0A4F55/#DDF1F0, Plum #7B3FC4/#3E1F73/#EEE5FA,
Moss #3B8A4A/#1F4D2B/#E1F2E4, Saffron #F0A216/#7A4A06/#FDF0D3. Modes: Light, Dark, Automatic. Dark values to be derived.

## Shape
- Window radius 14, buttons 10 (rounded rectangles, not pills), chips 8, covers 5 to 8, floating player 18, cards/banners 16 to 18.
- Round only: play/pause, switches, avatars.

## Elevation and button recipe (the "premium" feel; implement as reusable styles, not per-call literals)
Primary button: vertical gradient accent-top to accent, 1px top highlight (white at 40%), 1px bottom shade, 0.5px outer ring
in accent-deep, tight shadow 0 1px 2px accent-deep at 35%, soft coloured glow 0 6px 14px accent at 32%.
Secondary button: gradient button-top to button-bottom, 1px white top highlight, 0.5px outer ring ink at 14%, 0 1px 2px ink 8%.
Dark play button: gradient ink-top to ink, white 22% top highlight, 0.5px ring, 0 2px 4px + 0 6px 14px ink shadows.
Cover: 0 1px 2px ink 12% + 0 8px 20px ink 16%.
Floating player: translucent (white 78%) with blur, 1px white 85% border, 0.5px ring, layered shadows 0 1px 2px, 0 8px 20px, 0 24px 48px (ink 6/12/14%).
KNOWN ISSUE: the Wonder mock-up's button shadows show rough edges at some sizes (stacked inset + ring + glow).
The implementer must work out a clean pattern for GPUI (which may not support inset shadows natively: verify, and emulate the
highlight/shade with gradient overlays or borders), make it pixel-clean at 1x and 2x, and centralise it in one elevation module.

## Layout
App window 1440x900 min. Sidebar 220 wide (white, 1px right hairline), traffic lights in a 52px title area, content column with a
52px toolbar (title left; Explore button, sort control, search bar right). Search bar 320x34, radius 10, white, inset hairline ring,
placeholder "Search titles, authors, narrators" (Explore: "Search the Audible catalog"), shortcut hint Cmd-F.
Floating player: absolute, left/right 24, bottom 20, height 76, radius 18; left cover 48 + title/chapter; centre back-30, play/pause (36 dark circle),
forward-30 + scrubber row (elapsed, track 4px, remaining); right speed chip, sleep, chapters, volume.
Sidebar groups: [Search, Explore] / "Your library": Library, In progress, On this Mac, Bookmarks / "Browse": Authors, Series, Finished /
"Collections": user lists / footer "Signed in to Audible US. Library synced at 9:32 PM." Selected row: accent-tint bg, accent-deep medium text, radius 8, row height 28.

## Screens
1. Library: hero for the most recently played book (cover 220, serif title 44/-2% tracking, context line "Picked up from your iPhone, 41 minutes ago",
   one serif sentence "Nine minutes left in The Glass Orchard, and about nine hours in the book.", primary Resume + secondary Chapters),
   "Still listening" shelf (6 covers, 160 tall) and "Added this month" shelf (8 covers, 118 tall).
2. In progress: summary line, then 5 rows: cover 88, serif title 20, credit, "Chapter N of M, name", chapter-segmented progress bar (done / current / rest, 5px, 3px gaps),
   right: percent in Geist Pixel 30, time left, last device and when; Resume button (primary for the most recent, secondary for the rest).
3. Explore: credits chip, featured banner (cover colour gradient, cover 168, title 38, blurb, "Get with 1 credit" light button + "Listen to sample"),
   "Browse by genre" 6 tinted tiles, "New and noteworthy" 6 covers with price.
4. Now Playing: whole window gradient from cover colour; cover 420; title 24 serif; HUGE time-left numeral "8:57" in Geist Pixel ~168px;
   caption "left in The Glass Orchard, chapter 14 of 32."; seek bar; bottom row: speed + sleep ("Stop after this chapter"), back-30 / play (72, light gradient) / forward-30, bookmark + volume.
5. Mini player: floating 420x132 window (cover 104, title, chapter, scrubber, time left, controls, speed, expand) and a menu bar extra (headphones + pixel time)
   opening a 340 popover (cover 64, scrubber, controls, "Open Better Audible", "9 hr left in book").
6. Appearance settings: Mode cards (Light, Dark, Automatic), accent theme list with 4-colour chips, "Tint Now Playing with the cover" switch.

## Content rules
Plain sentence-case copy, no exclamation marks, no emoji. Times use tabular figures. Use realistic sample data until the Audible backend is wired.
