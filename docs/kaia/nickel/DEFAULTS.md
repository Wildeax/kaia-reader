# Nickel defaults on Libra Colour

The device reference is the owner's Kobo Libra Colour running firmware
4.42.23303, captured on 2026-10-14 at 1264 × 1680 pixels. The owner reports
unchanged Nickel reading defaults. The earlier static firmware study used
4.45.23697. Its predicted renderer conversions do not override these captures.

The study follows [PARITY.md, section 1](PARITY.md#1-scope-and-sources).
Firmware was read as data, never executed. Font inspection uses names and
metrics only. No firmware code, proprietary fonts or captured images belong
in this repository. The goldens contain Kaia's own render of the test book.

**Confirmed from device capture** below means measured visible behavior,
not an inferred internal setting. Font identity remains **Likely**. Sleep,
refresh and other unpictured defaults retain the static study's confidence.

## Capture measurements

The reference is `reader-default-body.png`, with `reader-open.png` showing
the following screen and `reader-menu.png` confirming its reading position.
The book is `test/juliet.epub`, About chapter. Both screens show book page
3 of 96 and chapter page 1 of 2, despite containing different text.

Measurements use native pixels with ink darker than gray 100. Ink bounds are
inclusive; layout right edges are exclusive. Antialiasing and glyph overshoot account for about 1 px of variation.
The first line's `a` and `e` give the lowercase height; `A` and `S` give the
capital height. Glyph bounds are not CSS em sizes.

| Measurement | Nickel, confirmed from device capture | Kaia headless render |
|---|---|---|
| Body lines | 9 | 9 |
| Baseline pitch | 140 px | 140 px |
| Lowercase ink height | 43 px, y=241 through 283 | 43 px, same y range |
| Capital ink height | 55 px for A, 57 px for S | 55 px for A, 57 px for S |
| First-line ink | x=144 through 1119; y=225 through 299 | x=149 through 1116; y=225 through 300 |
| Text column | About x=144 to 1120, 976 px wide | Layout x=147 to 1117, 970 px wide |
| Body outer ink | x=144 through 1119 | x=144 through 1116; Noto's W/A overhang the layout edge |
| Last-line ink | y=1345 through 1419 | y=1345 through 1420 |
| Header ink | x=538 through 723; y=50 through 69 | x=592 through 670; y=49 through 69 |
| Footer ink | x=446 through 819; y=1612 through 1631 | x=537 through 726; y=1609 through 1636, including lowercase descenders |
| Header/footer centre | Near y=60 / 1622 | Strips centred at y=60 / 1620 |

Nickel's body bands start at y=225, 365, 505, 645, 785, 926, 1065, 1205 and
1345. The sixth line has no ascenders reaching the usual top, so its ink
starts 1 px lower. This does not change the baseline pitch.

The old Kaia `reader-body.png` capture shows Act I's prologue, a different
passage with verse CSS. It has 17 body lines, 64 px pitch, 26 px lowercase ink
and 34 px caps. Body ink spans x=162 through 1146 and starts at y=326 below
the chapter headings. Its footer ink is at y=1639 through 1671 and says
page 6 of 248. Its verse indentation is not the About paragraph's margin.

`reader-font-settings.png` confirms publisher/default font and alignment off.
The slider track runs approximately x=519 through 1113. Knob centres at
761, 737 and 747 put font size at 40.7%, line spacing at 36.7% and margins
at 38.4%. These are screen positions, not direct pixel-size conversions.
The visible font list includes Atkinson Hyperlegible, Bitter, Noto Sans,
OpenDyslexic and Rakuten Serif. A listed face is not proof that the book uses it.

## Font choice and calibrated layout

The body has bracketed serifs, a two-storey a and g, and a large lowercase
height. Rakuten Serif remains a likely default-serif resolution from the
static study. The capture does not identify it conclusively. That font's
metadata gives 1000 units per em, x-height 510 and cap height 690. Noto Serif
has x-height 536 and cap height 714. FreeSerif has 450 and 662. Noto Serif
is the closest tested bundled open fallback for both glyph height and wrapping.
It ships under SIL OFL 1.1 at `resources/fonts/noto/LICENSE`.

The measured Noto Serif render at 76 px matches the capture's 43 px lowercase
ink and 55 to 57 px capital ink. Nominal em ratios alone underpredict the raster
bounds. Trials at 77 to 80 px changed more line breaks; FreeSerif at 90, 92 and 94 px did
not improve the passage match. No font file was added or replaced.

Both EPUB and KEPUB now use the same 36-unit fallback, or 76 px on Libra Colour.
The earlier 19-unit KEPUB fallback was a cross-layer WebKit inference and is
removed. The renamed EPUB fixture checks that Kaia does not make KEPUB smaller.
An independent Nickel KEPUB capture is still needed to confirm its renderer.

The line multiplier stays 1.35. CREngine applies it to Noto Serif's font
metrics, about 1.362 em, producing 140 px. It does not produce `76 × 1.35`
pixels. The round-one EPUB pitch therefore already matched the capture.

Side margins are 33.5 units, or 71 px. Juliet's publisher CSS adds 1 em on each
side, giving `71 + 76 = 147` px before glyph bearings, 3 px inside the reference
column on each side. A one-pixel sweep from 64 through 76 px found that 71
and 72 px reproduce all nine reference line breaks. The 71 px choice keeps
the column closer to the measured width. Setting the reader's
page margin alone to 145 px would count the publisher margin twice.
Top and bottom margins are 30 units, or 64 px. Each reading strip reserves
120 px, leaving a 1264 × 1440 document viewport. The first body ink falls at
y=225 and nine 140 px lines fit. The strips use 28 px Noto Sans with about
20 px caps. A single header item is centred; two groups retain their existing
left/right placement.

Alignment remains publisher-controlled, with `justify=false` meaning no
forced alignment override. Embedded styles stay enabled. Juliet's paragraphs
specify justified text. Fresh app layouts disable automatic dictionary
hyphenation, matching the observed passage. This boolean does not implement
a separate CSS-controlled automatic-hyphenation mode. Explicit saved choices
remain unchanged; manual hyphens in book text remain available for line breaks.

## Passage comparison and page counts

The headless app opens the unmodified book and selects zero-based screen 5,
its sixth rendered page. That screen starts at the same word as Nickel's
captured page 3. No fixture-specific CSS, text edits or manual breaks are used.

| Line | Nickel | Kaia |
|---|---|---|
| 1 | translated into every major | translated into every major |
| 2 | living language, and are | living language, and are |
| 3 | performed more often | performed more often |
| 4 | than those of any other | than those of any other |
| 5 | playwright. Shakespeare | playwright. Shakespeare |
| 6 | was born and raised in | was born and raised in |
| 7 | Stratford-upon-Avon. At | Stratford-upon-Avon. At |
| 8 | the age of 18 he married | the age of 18 he married |
| 9 | Anne Hathaway, who bore | Anne Hathaway, who bore |

All nine lines match exactly. The Kaia column is 970 px wide, 6 px narrower
than the reference, to compensate for differences in font advances and
CREngine's space condensation. Both next screens begin
`him three children:`. The defaults do not claim exact typography parity.

Kaia renders **1090 physical pages** for the whole book in both extension tests.
Nickel displays **96 pages**. The B-147 logical page model below now supplies
96 as Kaia's displayed total while preserving all 1090 screen turns.

The default header shows the chapter and its logical page count. The footer
shows the book title and logical book page count. The 120 px strips and 64 px
vertical margins remain unchanged.

## Defaults and Kaia mapping

Global font and margin values use the existing KOReader unit convention,
`ceil(units × min(screen width, screen height) / 600)`. Per-book layout values
are pixels. Old values below refer to the app before the Nickel defaults work.

| Setting | Nickel default | Confidence | Kaia key or field | Old Kaia default | New Kaia default |
|---|---|---|---|---|---|
| Font choice | Publisher/default | Confirmed from device capture | `reading.font_family` | Unset | Unset; Noto Serif fallback |
| Font size | 43 px lowercase ink, 55 to 57 px caps | Confirmed from device capture | `reading.font_size` | 22 units, 47 px | 36 units, 76 px for EPUB and KEPUB |
| Line spacing | 140 px pitch | Confirmed from device capture | `reading.line_spacing` | 1.0 | 1.35 |
| Side inset | About 144 px including publisher margin | Confirmed from device capture | `reading.margin_left`, `reading.margin_right` | 10 units, 22 px | 33.5 units, 71 px plus publisher margin |
| Vertical spacing | First ink y=225, last ink through y=1419 | Confirmed from device capture | `reading.margin_top`, `reading.margin_bottom` | 15 units, 32 px | 30 units, 64 px inside 120 px bars |
| Justification | Alignment off, book CSS justifies this passage | Confirmed from device capture and fixture CSS | `layout.justify` | false | false |
| Hyphenation | No automatic word splits in the reference passage | Confirmed for this capture only | `layout.hyphenation` | true | false for fresh app layouts |
| Publisher styles | Default font and alignment preserve book styles | Confirmed for this fixture | `layout.embedded_styles` | true | true |
| Full refresh | Chapter, not a fixed N; numeric choices 1 through 10 | Confirmed | `reading.full_refresh_pages` | 0, chapter | Unchanged |
| Page-turn waveform | Ghost-reducing partial update on supported panel | Confirmed | `reading.page_turn_waveform` | `cleaner` | Unchanged |
| Tap zones | Default preset | Confirmed preset; geometry not established in this pass | `reading.tap_zones` | `default` | Unchanged |
| Page-turn gestures | Mode 0, interpreted as taps and swipes | Likely | Reader gesture handling | Taps and swipes | Unchanged |
| Page buttons | Standard direction, not inverted | Confirmed | `reading.invert_page_buttons` | false | Unchanged |
| Sleep | 5 minutes | Confirmed | `device.sleep_minutes` | 15 | 5 |
| Power off | 0, never | Confirmed | `device.power_off_minutes` | 0 | Unchanged |
| Sleep cover | Enabled | Confirmed | `device.sleep_cover` | true | Unchanged |
| Sleep screen | Book cover, full-screen on color devices, information panel enabled | Confirmed | `device.sleep_screen` | `cover` | `details`; cover fitting and panel design differ |
| Automatic natural light | Disabled on this color device; bedtime 23:00, maximum bedtime warmth 92 | Confirmed | `display.natural_light_enabled`, `display.natural_light_bedtime`, `display.natural_light_bedtime_warmth` | false, 1380, 92 | Unchanged |
| Ambient light adjustment | Device has no ambient-light sensor | Confirmed device capability | No matching setting | Unsupported | Unchanged |
| Frontlight intensity | Missing-key fallback 80 on color devices | Confirmed | Platform state, no Rust preference default | Current hardware intensity | Unchanged; startup hardware state is not a missing preference |
| Orientation lock | Unlocked | Confirmed | `display.rotation_lock` | false | Unchanged |
| Header | Chapter title and chapter page progress; page progress enum 3 | Confirmed | `reading.header_chapter` | Header items off | Chapter title and logical chapter page count, centred |
| Footer | Book title and book page progress; page progress enum 6 | Confirmed | `reading.footer_page` | Pages on | Book title and logical book page count |
| Time left | Off in both chapter and book progress choices | Confirmed | `reading.footer_time_chapter`, `reading.footer_time_book` | false | Unchanged |
| Clock/battery in reading bars | No equivalent enabled default established | Likely | Header/footer clock and battery keys | false | Unchanged |
| Library view | Grid on color devices | Confirmed | `library.layout` | `covers` | Unchanged |
| Library sort | Recent sorter, with secondary ordering | Confirmed | `library.sort` | `title` | `read`, closest existing sorter; unread tie-breaking needs device comparison |


The power fallbacks come from `PowerSettings::getAutoSleepMinutes`,
`getAutoOffMinutes`, `getShowCover`, `getFullscreenCoverStretch` and
`getFullscreenCoverInfoPanel`. The library uses the grid and Recent branches
for this device class. The static study's font setting 40 and line-spacing
step 1.35 agree with the visible slider positions. Its 5% margin prediction
is insufficient to explain the visible scrubber without renderer behavior.

Nickel's chapter refresh branch clears after more than four turns. Kaia clears
on each chapter transition. This change does not alter that policy.

## Saved values and verification

Missing or invalid global values take the defaults. No migration overwrites
stored preferences. `BookSession` preserves saved per-book layouts, including
font size, margins, spacing, hyphenation and alignment. The Lua-profile importer
retains its source defaults of 22 font units, 10 side-margin units, 15 vertical
margin units, 1.0 spacing and automatic hyphenation. The Lua app is unchanged.
The strip geometry is shared reader UI. Existing books can repaginate within
the shorter viewport even though their saved numeric layout values stay intact.

The round-one engine changes remain: the CREngine size limit is 340 px, as in
KOReader's wrapper, and bookmark lookup includes block padding. Explicit saved
sizes above the former 72 px limit now render at their requested size.

`nickel_defaults.rs` checks the fresh headless app at 1264 × 1680, the saved
layout, the nine extracted visual lines, 140 px pitch, 147 px layout inset,
1090 physical pages, the next screen's opening words, EPUB/KEPUB equality and
saved global/per-book precedence. Its two goldens show the captured About
passage. Other changed reader goldens retain their original scenario and
reflect the shorter document viewport, hyphenation default and reading bars.
Every replacement golden must be visually inspected before acceptance.

Run the comparison from `rust/` after loading `tools/kaia/env.sh`:

```sh
cargo test -p kaia-app --test nickel_defaults -- --nocapture
```

## B-147 logical EPUB pages

EPUB and KEPUB display labels now use the Adobe compressed-content rule.
Each OPF spine item contributes `max(1, ceil(compressed ZIP bytes / 1024))`
pages, and the book total is their sum. Kaia opens the container's declared
OPF and reads compressed sizes from the ZIP directory. It does not estimate
sizes from extracted text or compress the content again.

Kaia caches CREngine's `DocFragment` boundaries on the engine thread at open
and after each reflow. Reading page labels never waits for the engine.
Within an item, the fraction is `(current screen - first screen) / item screen count`.
The one-based item label is `1 + floor(fraction * item logical pages)`, clamped
to the item's last page. The book label adds the preceding items' page counts.
This is a screen-resolution approximation of the reading position within the
item. Font changes can move a position near a logical boundary to the other
side; compressed page totals remain unchanged. The current implementation
uses spine items as chapter boundaries for these labels.

For `test/juliet.epub`, the total is 96 and About spans book pages 3 and 4.
At the calibrated layout, About occupies zero-based screens 3 through 17.
Screens 3 through 10 show logical page 3; screens 11 through 17 show page 4.
The two captured passages on screens 5 and 6 both show About 1 of 2 and book
page 3 of 96. The EPUB and KEPUB integration test checks every About screen.

Screen navigation, saved percent, reading statistics and time estimates retain
their existing basis. Fixed-layout documents retain document-page counts.
Malformed EPUB metadata or an unavailable fragment position falls back to
screen labels. Adobe margin markers remain unimplemented.
