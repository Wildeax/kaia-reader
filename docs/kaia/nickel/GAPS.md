# Rust app gaps against the Nickel captures

Recorded on 2026-10-14 for B-103. [SCREENS.md](SCREENS.md) gives measured descriptions for all 63 current Nickel captures, plus the temporary duplicate inspected before removal. This comparison uses all 24 PNGs present in `/Users/wildeax/kaia-device-shots/kaia-rust/` at the inventory pass. Each was opened and inspected. The inventory below names them, including two captures whose requested panels were not visible. The temporary Nickel `x.png` was also inspected before it was removed from the input directory.

Nickel is Spanish and Kaia is English in these captures. Compare control meaning and hierarchy, not literal translations or text width alone. Use Kaia's own short labels, open icons and licensed fonts. Preserve the owner's B-103 service exclusions and the other owner decisions, including no telemetry, opt-in network actions and scoped resets. Do not copy Kobo assets, long strings, proprietary fonts, dictionary entries or the accidentally imported junk covers into the repository.

## Evidence, sizing and order

Rows below are ordered by their effect on reading, then book discovery and device setup. V means a visible comparison between the supplied images. N means an action/result reported in the external device-sweep `NOTES.md`, not a gesture test performed by this researcher. C means a read-only source inspection supports the difference but a Kaia capture is missing. U means the reference is visible but the Kaia outcome or behavior still needs a device capture. A row can combine these forms of evidence. A missing capture is not proof of a missing feature. Source files were read while other workers were active, so their contents may already differ from the captured build.

Sizes follow the parity plan: S is a local change to one screen/module; M spans two to four modules or introduces a screen; L needs a subsystem or device measurements before implementation can be settled. They estimate the described change, not a promise of elapsed time. Every code path below exists under `rust/kaia-app/src/surfaces/`; that directory is the prefix for shortened paths such as `reader/panels.rs`. The file named first is the most likely app-level entry point. Some rows also need shared widgets, input, document or platform work outside this directory. Those dependencies are named without claiming the app file can solve them alone.

Coordinates refer to the 1264 x 1680 device screenshots. Layout bounds are approximate; font sizes are visible cap or x-height, not nominal font size. Do not multiply every Kaia UI font by the body-text ratio. Nickel's body text is much larger, but its menus and controls are often smaller and denser than Kaia's.

## Kaia capture inventory

| Captures inspected | Visible state and limits |
|---|---|
| `home.png`, `home-2.png` | Home before/after reading; same layout, changed progress and cover height. |
| `library.png` | Three-book cover grid; no dropdown or list state. |
| `reader-open.png`, `reader-body.png` | EPUB cover and verse, not the identical Nickel passage. |
| `reader-menu.png`, `text-layout.png`, `contents.png` | Reader bars, typography sheet and chapter list. |
| `reader-hold.png`, `reader-dictionary.png` | Selection sheet and separate no-dictionary state. |
| `reader-search.png` | Empty in-book search with keyboard. |
| `light.png`, `reader-notes-panel.png` | Reading pages only. Neither requested panel is visible. Do not treat these as panel evidence. |
| `notes.png` | Global annotation library, empty. This is not a notebook list. |
| `settings.png`, `settings-page2.png` | Both pages of the settings index. |
| `settings-reading.png`, `settings-display.png` | Page one of Reading and Display/light. Later pages are absent. |
| `settings-device.png`, `settings-network.png` | Page one of Device and Network. Later pages are absent. |
| `settings-home.png`, `settings-library.png` | Home section switches and local library scanning preferences. |
| `settings-about.png`, `settings-stats.png` | App information and page one of statistics. No matching Nickel statistics capture: `more-activity.png` actually shows notebooks. |

## Reading page and default typography

Nickel references are `reader-default-body.png`, `reader-open.png` and `reader-default-titlepage.png`. Kaia references are `reader-body.png`, `reader-open.png` and the mislabeled `light.png`. The body captures show different sections of the same titled book. Kaia's open capture is the cover, while Nickel's open capture is prose. Neither is an exact passage-to-passage comparison.

| Id | Evidence and difference | Concrete change for Kaia | Size | Likely source files |
|---|---|---|---|---|
| R01 | V/C. Nickel body capitals measure 57 px and x-height 41 px. Kaia measures 34 px and 26 px. Kaia's text panel reports size 47; Nickel exposes no numeric size. | Increase the default document text size until the same EPUB passage measures about 57 px caps and 41 px x-height with Kaia's open font. Start calibration around 1.6 times the captured size, but choose the saved value from glyph measurements, not a guessed Nickel point size. Apply to fresh layouts; preserve explicit saved user choices. | M | `reader/mod.rs`, `reader/state.rs`, `settings/keys.rs` |
| R02 | V. Nickel has about 140 px line pitch and nine prose lines. Kaia's verse body has about 64 px pitch and 17 body lines below its headings. | Set font size and line spacing independently to match the 140 px reference pitch for the same prose passage. Check book paragraph styles and reflow before choosing a default multiplier. Do not derive a global words-per-line setting from unlike prose and verse. | M | `reader/mod.rs`, `reader/state.rs`, `reader/panels.rs` |
| R03 | V/C. Nickel prose ink spans x=143 to 1121. Kaia verse starts around x=163 and extends near x=1147; speaker labels in `light.png` start near x=115. Kaia's panel says margins 22, so visible insets are not just that setting. | Match the same EPUB's total content inset and available width, including CSS indentation, instead of setting every reader margin to 143. Reserve the reference header/footer areas before layout. Capture both apps at the identical passage to separate reader margins from book styles. | L | `reader/mod.rs`, `reader/page.rs`, `reader/state.rs`; document layout also needs inspection |
| R04 | V/C. Nickel shows a centered chapter/progress header at y=50 to 69. Kaia shows no persistent header; `reader/header.rs` defaults all header items off. | Add a default centered chapter title plus chapter-page status, with 20 px caps at y=50. Replace the existing left/right header composition for this mode. Preserve selectable alternatives in reading settings. | M | `reader/header.rs`, `reader/page.rs`, `reader/state.rs` |
| R05 | V/C. Nickel's footer is black, has 20 px caps at y=1612 to 1631, and includes the book title and page fraction. Kaia's gray page-only footer has about 25 px caps near y=1639 and sits lower. | Move the footer ink up about 27 px, reduce its type to the measured 20 px cap height, use black, and add the book title before the configured book-progress field. Center the complete line and handle long titles. | M | `reader/footer.rs`, `reader/page.rs` |
| R06 | V. Nickel keeps a full-width 1 px progress track at y=1663 and a 5 px fill. Kaia has no visible page-edge progress strip. | Add the persistent book-progress track at y=1663 with the fill centered vertically on it. Expose its on/off control in reading settings, on by default. Keep it separate from the menu seek slider. | M | `reader/page.rs`, `reader/footer.rs`, `settings/sections.rs`, `settings/keys.rs` |
| R07 | V/U. Nickel shows 3 of 96 on two different rendered prose screens and an Adobe marginal marker on the title page. Kaia shows 6 of 248 on the verse page. The settings name remaining-page modes, but rendered text is a fraction. | Distinguish screen turns, chapter progress, book progress and Adobe page labels in the reader model. Verify the same EPUB location and enabled settings before changing page totals or claiming an off-by-one error. Add the Adobe margin label in the correct reserved area. | L | `reader/state.rs`, `reader/header.rs`, `reader/footer.rs`, `reader/page.rs`; document page mapping is a dependency |
| R08 | V/C. Nickel prose is justified while its alignment override is off. Kaia's panel has Justified text off and Book's styles on; the verse is ragged right. The images do not prove that Kaia ignores justification. | Replace the binary alignment override with three choices: use book alignment, left and justified. Preserve author-styled verse. Verify the reference prose under book styling before treating justification as a renderer defect. | M | `reader/panels.rs`, `reader/commands.rs`, `reader/state.rs` |
| R09 | V/U. Nickel's title-page capture includes title, author, publication metadata, publisher artwork and a margin page marker. Kaia's open capture is a cover image at about `130, 89, 1004, 1489`. These are different book locations. | Preserve resume/open location and compare the same title page in a later pass. Do not replace Kaia's cover with a fabricated title page or copy the publisher artwork. Check title-page scale, author-style preservation and image fitting at matching locations. | L | `reader/state.rs`, `reader/navigation.rs`, `reader/page.rs`, `reader/persistence.rs` |

The observed body gap is about 1.68 times the cap height, 1.58 times the x-height and 2.19 times the line pitch. These ratios are useful starting measurements, not font-size API values. A clean comparison needs identical book bytes, paragraph, orientation, saved-layout state and display settings.

## Reader menu

Compare Nickel `reader-menu.png` with Kaia `reader-menu.png`. Both leave the book behind the bars. Nickel's page stays at the same coordinates as `reader-open.png`, and the bottom bar hides the last line. Kaia likewise occludes book text. Keep this distinction from reflowing the book merely because the menu opens.

| Id | Evidence and difference | Concrete change for Kaia | Size | Likely source files |
|---|---|---|---|---|
| M01 | V. Kaia's top bar is y=0 to 157 with a gray frame. Nickel has a small clock/battery row plus an action row ending at y=178. | Build two rows. Put the clock at `50, 34` and battery near `1171, 29`; end the toolbar with a black 1 px rule at y=178. Remove the full gray frame. | M | `reader/overlay.rs`, `reader/footer.rs` |
| M02 | V. Kaia uses a Home icon above a label at the left and gives much of the bar to the book title. Nickel uses a left arrow plus a bold return-home label at x=107. | Replace the icon-over-label Home group with the left arrow and one-line return label. Remove the large book-title field from the menu bar; keep the title in the persistent footer and book dialogs. | S | `reader/overlay.rs` |
| M03 | V/C. Kaia puts Search, Bookmark and More across the top. Nickel puts pen, rotation, light, font, settings and More at x=750, 842, 932, 1018, 1104 and 1199. | Rebuild the right toolbar in that order with Kaia's open icons. Move search to the lower-right row; move light and font up from the bottom. Add direct pen, orientation and reading-settings actions. Retain the top-right bookmark gesture reported in the device-sweep notes and provide access through annotations. | M | `reader/overlay.rs`, `reader/mod.rs`, `reader/commands.rs`, `settings/quick.rs` |
| M04 | V. Kaia puts bold labels under every reader tool. Nickel uses icon-only tools, mostly 44 to 52 px ink, with black circular active states of about 70 px. | Remove the visible subordinate labels from reader toolbar tools, size the icon ink to the reference and add black/white selected states. Keep accessible control names and sufficient hitboxes separately from icon bounds. | M | `reader/overlay.rs`, `reader/panels.rs` |
| M05 | V. Kaia's lower panel starts at y=1312; Nickel starts at y=1344. Kaia's position text is to the slider's right and repeats below it. | Move the lower boundary down 32 px. Show one centered position label near y=1410, above the slider. Remove the repeated page line from the lower menu. | S | `reader/overlay.rs` |
| M06 | V. Kaia uses a roughly 70 px filled black slider thumb and a 12 px gray track from x=101 toward 844. Nickel uses a roughly 48 px outlined knob and a 1 px track from x=130 to 1133 at y=1512, with a 5 px filled part. | Resize and restyle the menu slider, extend its track, and center it at y=1512. Add the two edge-navigation icons near x=84 and 1180. Verify whether those icons seek by chapter or to book boundaries before binding their actions. | M | `reader/overlay.rs`, `reader/navigation.rs`, `reader/commands.rs`; shared slider drawing also changes |
| M07 | V/C. Kaia's bottom row is Contents, Notes, Text and layout, Light. Notes routes to the global notes screen. Nickel has contents plus the current chapter on the left, then this book's annotations and search on the right. | Replace the four equal labeled cells with the reference grouping near y=1605. Route the annotations icon to the current book. Keep global notes in the app navigation or More. | M | `reader/overlay.rs`, `reader/panels.rs`, `reader/mod.rs` |
| M08 | U/C. Kaia source closes the overlay on an unused tap. The reference stills do not establish its dismissal hitboxes, page-key behavior or seek undo. | Retain the current page under both bars, then test center taps, outside taps, button turns and seek return against Nickel. Adjust event routing only from those observations. Do not claim behavior parity from the visual rebuild. | L | `reader/overlay.rs`, `reader/page.rs`, `reader/navigation.rs` |

## Font panel

Compare Nickel `reader-font-settings.png` with Kaia `text-layout.png`.

| Id | Evidence and difference | Concrete change for Kaia | Size | Likely source files |
|---|---|---|---|---|
| F01 | V. Kaia's rounded sheet fills y=9 to 1680, with a large title and cross. Nickel's font panel occupies y=179 to 740 beneath the toolbar, without its own heading or cross. | Replace the full-screen sheet with a top-attached panel 562 px high at this resolution. Keep the toolbar and book preview visible; use the font tool's active state and verified outside-tap behavior to close it. | M | `reader/panels.rs`, `reader/mod.rs` |
| F02 | V. Kaia's title has about 51 px total ink height and label capitals about 29 px. Nickel's compact sans-serif labels have 24 to 25 px caps at x=85. | Remove the redundant large heading. Reduce control text to the reference cap height and move the label column from x=48 to x=85. Use a layout-specific text style instead of shrinking all application text. | S | `reader/panels.rs` |
| F03 | V. Kaia uses large outlined minus/plus boxes, about 104 x 104 px, and centered numeric values. Nickel uses bare endpoint marks and three thin sliders at y=372, 465 and 557. | Replace the Size, Line spacing and Margins steppers with tracks from x=519 to 1112, 48 px ring knobs, and endpoint marks near x=456 and 1175. Remove persistent numeric readouts from these rows. Preserve usable touch targets around the small ink. | M | `reader/panels.rs`, `reader/commands.rs`, `reader/state.rs` |
| F04 | V. Kaia's font choice is a thick rounded button at `923, 362, 285, 94`. Nickel has a square dropdown at `338, 239, 573, 80` and a separate Advanced action to its right. | Move the font choice to the first compact row, add a down chevron, use a 2 px square outline, and add Advanced near x=962. Label the first choice in Kaia's words as the book font. | M | `reader/panels.rs`, `reader/commands.rs` |
| F05 | V. Kaia exposes weight, hyphenation, hyphenation language and Book's styles in the main sheet. Nickel's main panel ends with a three-part alignment selector at y=609. | Move weight into Advanced and move Kaia-specific hyphenation/style controls into a secondary settings route. Add the three equal alignment segments in `428, 609, 776, 83`. Keep existing functions reachable while matching the primary panel. | M | `reader/panels.rs`, `reader/commands.rs`, `reader/mod.rs` |

## Font list

Nickel reference is `reader-font-list.png`. There is no Kaia font-list capture. Source defines a separate `font_list` builder and includes book, Noto, Atkinson and OpenDyslexic choices plus user fonts.

| Id | Evidence and difference | Concrete change for Kaia | Size | Likely source files |
|---|---|---|---|---|
| F06 | C/U. Kaia's existing list is built as a reader sheet; Nickel opens an anchored dropdown over the still-visible font panel. | Rebuild the font picker as a square popup near `347, 248, 628, 1202`, constrained to the available choices and screen. Use 91 px row pitch, a check at x=403, labels at x=448, hairline rules and a lower scroll affordance. Capture the result before claiming visual parity. | M | `reader/panels.rs`, `reader/mod.rs` |
| F07 | U. Nickel previews each face in its own row font. A Kaia screenshot is needed to compare this behavior. | Render each licensed font name in its own face, at about 24 to 29 px cap height, with the current face checked. Keep the document-font choice first and user fonts available. Do not add proprietary names as nonworking choices. | M | `reader/panels.rs`, `reader/state.rs` |

## Advanced font dialog

Nickel reference is `reader-font-advanced.png`. Kaia has no corresponding capture; its visible weight control is an inline stepper in `text-layout.png`.

| Id | Evidence and difference | Concrete change for Kaia | Size | Likely source files |
|---|---|---|---|---|
| F08 | V/C/U. Kaia's captured sheet has no Advanced entry or before/after preview. | Add a 25 px inset dialog with face, size and weight controls, a two-column preview at `100, 710, 1064, 744`, and Restore/Apply actions near y=1524. Use Kaia-authored sample text and open fonts. Separate preview changes from applied settings and test cancel/restore behavior on device. | M | `reader/panels.rs`, `reader/state.rs`, `reader/commands.rs`, `reader/mod.rs` |
| F09 | V. Kaia uses 6 px rounded button outlines for its weight stepper. Nickel's Advanced uses 1 px tracks, 5 px fills, 48 px ring knobs and flat gray action buttons. | Reuse the compact slider treatment for size and weight. Use 80 px-high flat actions, about 24 px caps, and square preview borders. Keep the large comparison area instead of a single value label. | S | `reader/panels.rs` |

## Contents

Compare Nickel `reader-toc.png` with Kaia `contents.png`.

| Id | Evidence and difference | Concrete change for Kaia | Size | Likely source files |
|---|---|---|---|---|
| C01 | V. Kaia opens a rounded bottom sheet at y=505, leaving the toolbar and book behind. Nickel opens a 25 px inset dialog nearly filling the screen. | Replace the 70-percent sheet with `25, 25, 1214, 1630`, square corners, a 1 px black outline and a `#4d4d4d` surround. Remove the exposed toolbar/book from the dialog view. | M | `reader/panels.rs`, `reader/mod.rs` |
| C02 | V. Kaia uses a large Contents heading and three thick pill tabs, Chapters, Bookmarks and Highlights. Nickel centers the book title above two plain text tabs, contents and annotations. | Put the book title at y=71 with about 23 px caps. Replace the three boxed tabs with two text tabs near y=168, a y=219 rule and a 6 px active underline. Combine bookmark, highlight and note discovery in Annotations. | M | `reader/panels.rs`, `reader/mod.rs` |
| C03 | V. Kaia fits four chapter rows at about 166 px pitch, each with a secondary page label. Nickel fits 16 rows at 82 px pitch, without row page labels. | Reduce row pitch to 82 px and cap height to 23 to 25 px. Remove the secondary page-number line. Put top-level text at x=199 and children at x=270. Extend the list from y=220 to 1532. | S | `reader/panels.rs` |
| C04 | V/C. Kaia uses a Previous/Page 1 of 9/Next footer. Nickel has a right scrollbar with arrows and no bottom pager. Kaia source marks the current row with a trailing Now value; Nickel uses bold text. | Replace the bottom pager with the reference scrollbar near x=1167, retain full hierarchy, and mark the current section in bold. Verify scrollbar, swipe and hardware-button navigation before replacing the interaction model. | M | `reader/panels.rs`, `reader/navigation.rs`; shared list support may change |

## Annotations

Nickel reference is `reader-annotations.png`. Kaia's `contents.png` shows the alternative three-tab structure, but there is no Kaia capture of its bookmark/highlight rows or empty state.

| Id | Evidence and difference | Concrete change for Kaia | Size | Likely source files |
|---|---|---|---|---|
| C05 | V/C/U. Kaia separates bookmarks and highlights, and the toolbar Notes action leaves the current book. Nickel combines book annotations in the second tab. | Build one per-book annotations tab with a type filter near x=98, a position-sort control at `354, 246, 461, 64`, and overflow near x=1152. Include highlights, notes and bookmarks. Keep global Notes reachable elsewhere. | M | `reader/panels.rs`, `reader/mod.rs`, `notes/data.rs` |
| C06 | U. Nickel shows a two-line centered empty message near y=963 with 43 px pitch. Kaia's per-book empty state is uncaptured. | Use a brief Kaia-written empty-state sentence at that position and size. Capture empty and populated states, filter choices, sorting and edit/delete routes before specifying annotation row geometry. | S | `reader/panels.rs` |

Kaia `notes.png` shows the global notes library, while `reader-notes-panel.png` shows only a reading page. Neither supplies the missing per-book annotation view.

| Id | Evidence and difference | Concrete change for Kaia | Size | Likely source files |
|---|---|---|---|---|
| C07 | V. Global Notes has a large heading, five thick pill filters over two rows at y=225 to 442, an export action near y=521 and a left-aligned gray empty message near y=630. Its app navigation sits at y=753 to 872, leaving the bottom half blank. | Keep global notes distinct from the per-book tab. Anchor its application navigation to the bottom and move global export/filter controls into compact rows. Apply the per-book geometry in C05/C06 only to book annotations, not to every global notes state. | M | `notes/mod.rs`, `common.rs`, `mod.rs` |

## Reading settings and gestures

Nickel references are `reader-settings.png`, `reader-settings-2.png`, `reader-settings-3.png` and `settings-reading.png`. Kaia `settings-reading.png` shows page one of three: font choices, font-install instructions, size and button inversion. Pages two and three and the separate header/footer panel remain uncaptured. Other controls are spread across reading and display settings.

| Id | Evidence and difference | Concrete change for Kaia | Size | Likely source files |
|---|---|---|---|---|
| S01 | V/C/U. Kaia `settings-reading.png` spends y=317 to 798 on five font radios, y=863 to 928 on installation prose and y=943 to 1049 on size controls. Its reader More exposes Header and footer as a separate sheet. Nickel groups progress, buttons, appearance and touch settings. | Add one reading-settings dialog in the 25 px inset shell. Group page one as progress and buttons, page two as appearance, and page three as touch controls. Use 49 px gray section strips, about 106 px setting rows, 38 px checkboxes and right-aligned values. | M | `reader/panels.rs`, `settings/sections.rs`, `settings/section.rs`, `settings/entries.rs` |
| S02 | C/U. Kaia's header/footer sheet offers independent item switches. Nickel exposes one header mode, one footer mode and a separate progress-bar checkbox. | Replace the primary switch grid with two dropdown rows at y=329 to 540 and the progress checkbox below. Default to chapter pages left, book pages left and progress on. Keep extra Kaia fields as secondary choices rather than crowding the main panel. | M | `reader/header.rs`, `reader/footer.rs`, `reader/panels.rs`, `settings/keys.rs` |
| S03 | V/C/U. Kaia shows an off Invert page buttons switch at `1024, 1183, 160, 53` in `settings-reading.png`. Nickel illustrates two portrait buttons and uses a default/reversed segmented control. | Add a Kaia-drawn device diagram around `346, 911, 345, 388`, action labels to its right, and a selector at `136, 1368, 984, 90`. Keep default direction selected. Test orientation-dependent hardware mapping. | M | `settings/sections.rs`, `settings/entries.rs`, `reader/navigation.rs` |
| S04 | C/U. Kaia already encodes chapter refresh as zero and defaults button inversion off; this is not an absent feature. Orientation and night mode are placed elsewhere. | Keep chapter refresh and default button direction. Move refresh, automatic orientation, Adobe numbering, dark mode and color-artifact reduction into appearance rows matching page two. Default Adobe numbers on and dark mode off. Verify actual refresh and rotation on device; source defaults alone are insufficient. | L | `settings/sections.rs`, `settings/keys.rs`, `reader/page.rs`, `reader/state.rs`; platform and document support may be needed |
| S05 | C/U. Kaia's source lists three tap-zone presets. Nickel shows four diagram choices. No Kaia gesture settings capture exists. | Add all four illustrated presets, using original drawings at x=157, 421, 685 and 949, y=482. Match left-back/center-menu/right-forward as default. Put pinch sizing, left-edge brightness and tap-or-swipe choices below, with their reference defaults on. Measure actual zone boundaries on device before hardcoding diagram fractions as hitboxes. | L | `settings/keys.rs`, `settings/sections.rs`, `settings/entries.rs`, `reader/page.rs`; input mapping is a dependency |
| S06 | U/C. Kaia has gesture handling, but static images prove neither its thresholds nor parity with Nickel. | Run and record tap, swipe, pinch, edge-brightness, button-direction and long-press cases with a book open and with overlays open. Resolve conflicts such as brightness drags versus page turns. Add missing enable controls rather than assuming every implemented gesture is always wanted. | L | `reader/page.rs`, `reader/navigation.rs`, `reader/mod.rs`, `settings/keys.rs` |
| S07 | U. Nickel's settings entry from the index has a back arrow; entry from reading has only close. Kaia has no paired route captures. | Preserve the entry context. Back from the application route returns to the settings index; close from the reader route returns to the same reading position. Capture both routes after rebuilding the dialog. | M | `reader/mod.rs`, `settings/mod.rs`, `settings/section.rs` |

## Light

Nickel references are `reader-light.png` and `home-light.png`. Kaia `light.png` shows only book text and its footer, so the reader panel remains uncaptured. `settings-display.png` shows full-page light controls: brightness 0 percent, warmth 94 percent, Auto off and bedtime 23:00. These saved/hardware values are observations, not proof of factory defaults.

| Id | Evidence and difference | Concrete change for Kaia | Size | Likely source files |
|---|---|---|---|---|
| L01 | V/C/U. Kaia opens Light from the lower menu into general quick settings. That builder also includes Wi-Fi, airplane mode, orientation and sleep. Nickel has a dedicated light callout anchored to the top toolbar. | Move the reading light action to the top toolbar and add an anchored panel at `451, 182, 803, 869`. Keep general quick settings as a separate route. Include brightness and natural-light controls only in this reading panel. | M | `reader/overlay.rs`, `reader/panels.rs`, `settings/quick.rs` |
| L02 | V/C/U. Kaia leaves Auto and bedtime in Display and light. The capture shows scheduling off and bedtime 23:00; the source default for scheduling is false. Its 70 px filled knobs, 12 px tracks, 160 x 53 px toggle and 107 px steppers differ from the reference. | Add Auto beside the natural-light heading and the night-time dropdown below the warmth slider. Use the captured Auto-on, 23:00 reference for a fresh configuration. Preserve saved user choices and current hardware brightness. Use 26 percent as the comparison fixture, not an unexplained override every time a panel opens. | M | `settings/quick.rs`, `settings/sections.rs`, `settings/keys.rs` |
| L03 | U. Kaia's panel dimensions and live light behavior cannot be measured from the supplied shot. | Recapture with the panel visibly open. Match the 1 px/5 px tracks, 48 px ring knobs, 88 x 49 px Auto switch, 26 percent readout, divider positions y=524 and 949, and day/night endpoint icons. Test drag persistence, Auto/manual interaction and outside-tap dismissal. | L | `settings/quick.rs`, `reader/panels.rs`; slider and platform behavior also need checks |

## Selection and dictionary

Compare Nickel `reader-hold-word.png` with Kaia `reader-hold.png` and `reader-dictionary.png`. Selection and dictionary are captured separately in Kaia. Note entry remains uncaptured.

| Id | Evidence and difference | Concrete change for Kaia | Size | Likely source files |
|---|---|---|---|---|
| D01 | V/C. Kaia shows a full-width rounded selection panel at y=1301 to 1680, a word heading at `32, 1343`, and five thick outlined action buttons at y=1549 to 1642: Note, Dictionary, Copy, Search and More. Nickel anchors a small menu near the word and shows the dictionary separately. | Add a word-anchored callout, about 342 x 379 px in the reference position. Place four primary color dots across its top, then three rows for Highlight, Add note and Search. Put extra Kaia actions in a secondary route. Do not require a dictionary button press if device testing confirms Nickel's lookup opens on hold. | M | `reader/panels.rs`, `reader/mod.rs`, `reader/dict.rs` |
| D02 | V/C. Kaia shows nine roughly 102 px color disks centered at y=1455; source binds their taps to immediate highlights. Its selected word has gray fill near `519, 636, 82, 64`, without visible selection handles. Nickel shows four 55 px colors, a selection ring and a separate highlight row. The still does not prove when color choice commits a highlight. | Match the primary four-color row with 55 px disks and a visible selected ring; keep extra colors in secondary options. Verify color selection versus save behavior before changing the command binding. Use original selection handles and keep them outside the selected word. | M | `reader/panels.rs`, `reader/commands.rs`, `reader/page.rs` |
| D03 | V. Nickel shows a separate 1202 x 647 px dictionary card, language selector, result and lookup/expand actions. Kaia has a full-width rounded panel at y=1110, a large Dictionary heading near `48, 1192`, close cross, word heading, no-dictionary message with filesystem instructions and two 564 x 93 px actions at y=1533. Selection is not visibly retained behind it. No dictionary is installed in this captured build; lookup is not necessarily unimplemented. | Match the panel placement at `31, 1002`, selector near y=1047, result area below y=1103 and action strip below y=1558. Use about 25 px body caps with 45 to 48 px pitch. Preserve the selected word and both handles outside the card. Install an approved dictionary for the device comparison and test fallback results. Keep setup in the selector/settings route, with brief instructions instead of internal path details in normal lookup. | M | `reader/panels.rs`, `reader/dict.rs`, `reader/mod.rs` |
| D04 | U. Selection dragging, automatic lookup, search submenu, expansion and dismissal cannot be compared from a single Nickel still. | Use the captured hold state as a baseline, then capture drag, note, installed-dictionary hit/miss and expanded states and test returns to reading. Keep web/encyclopedia actions behind B-92 opt-in. Do not reproduce Nickel's dictionary definition or publisher wording as fixture text. | L | `reader/mod.rs`, `reader/panels.rs`, `reader/dict.rs`, `reader/page.rs` |

## Search and keyboard

Compare both `reader-search.png` captures. Kaia shows its English keyboard and empty search sheet; Nickel shows the scoped search dialog. Neither capture contains an in-book search result.

| Id | Evidence and difference | Concrete change for Kaia | Size | Likely source files |
|---|---|---|---|---|
| Q01 | V. Kaia has a rounded sheet from y=169 to the bottom, a large heading at `48, 249`, a close cross, a field at `54, 361, 928, 95`, a separate black Search button at `1011, 361, 198, 95`, instructions near y=502 and an empty-results pager at y=993. Nickel has an inset page with back, scope and book selectors. | Build the upper dialog at `25, 25` down to y=1040. Use a centered search title, back at x=62, and a query box `61, 137, 1142, 86`. Put scope inside the right end of the field and the current-book selector below. Remove the redundant large heading and empty-result pager from the entry state. Preserve search history without inventing unavailable scopes. | M | `reader/panels.rs`, `reader/mod.rs`, `reader/search_history.rs` |
| Q02 | V/C. Kaia has four rows of rounded boxed keys, about 105 px high at 115 px pitch, from y=1165 to 1616. Word labels identify Shift/Delete/Space/Hide/Enter. It has a 123 mode but no visible number row or caret arrows. Nickel has five unboxed rows starting at y=1040, with Go at lower right. | Move the keyboard top up 125 px and match the 640 px keyboard height, roughly 115 px column pitch, number row, caret arrows, accents/symbol controls and `985, 1553, 214, 98` submit key. Keep key labels readable at about 35 to 41 px ink height. Select an appropriate locale layout instead of unconditionally choosing English. | M | `reader/panels.rs`, `library/views.rs`; shared keyboard widget and localization also change |
| Q03 | U. The later Nickel library-search capture shows one result. Neither app has a supplied in-book results, active-hit or history capture. | Add a later device comparison for a successful query, no result, history recall, result jump and return. Verify caret, Shift, accents, Back and keyboard dismissal. Keep existing history behavior while reorganizing entry controls. | L | `reader/search_history.rs`, `reader/panels.rs`, `reader/navigation.rs` |

## Reader More

Nickel reference is `reader-more.png`. Kaia has no open-More capture. Its source uses a bottom sheet with eight secondary buttons, including ink, navigation, finish, header/footer, word list and plugins.

| Id | Evidence and difference | Concrete change for Kaia | Size | Likely source files |
|---|---|---|---|---|
| M09 | C/U. Nickel's menu is a four-row callout at the top right with book details, a local screenshot toggle, dictionary and help. Kaia's secondary actions use a bottom sheet. | Anchor the reader overflow near `713, 182, 541, 383`, with square 3 px borders, about 91 px row pitch, 25 px caps and hairline separators. Include relevant book details, dictionary and help routes. Move pen and reading preferences into their primary toolbar controls. Place remaining Kaia-only tools in a secondary group so they remain reachable. | M | `reader/panels.rs`, `reader/mod.rs`, `reader/commands.rs` |
| M10 | U. The screenshot action in Nickel is an installation customization, and its on/off state is not visible. | Keep screenshot access with Kaia's own existing feature if appropriate; do not copy an unverified toggle contract or remote tool into the main reader. Capture the actual Kaia menu and verify every route and dismissal. | S | `reader/panels.rs`, `reader/commands.rs` |

## PDF reading, zoom and pan

Nickel references are `pdf-open.png`, `pdf-page2.png`, `pdf-pinch-out.png`, `pdf-zoom-pan.png`, `pdf-zoom-tap-right.png`, `pdf-zoom-menu.png`, `pdf-move-tool.png`, `pdf-pinch-in.png` and `pdf-menu.png`. No Kaia PDF capture exists. The current source already offers Fit page, Fit width, Fit columns, Zoom to area and Reset. Do not label zoom support absent merely because it is not captured.

| Id | Evidence and difference | Concrete change for Kaia | Size | Likely source files |
|---|---|---|---|---|
| Z01 | U. Nickel `pdf-page2.png` and `pdf-pinch-in.png` show scaled page images without the EPUB title/progress text, retaining only the thin bottom progress strip. `pdf-open.png` is blank except for that strip; the cause is unknown. | Capture Kaia at the same PDF pages and scales. Match page fitting and chrome by document type, with no reflow font controls on scans. Investigate any actual blank render from a device trace; do not copy the blank opening state as intended UI. | L | `reader/page.rs`, `reader/state.rs`, `reader/footer.rs`; fixed-layout engine work may be needed |
| Z02 | N/U. The sweep reports pinch zoom around the fingers, about 2.5 to 3 times fit scale, and drag-to-pan. The `pdf-pinch-out.png` and `pdf-zoom-pan.png` crops corroborate scale and viewport changes. Kaia behavior is unmeasured. | Test and match finger-centered scaling, continuous drag and sensible page-edge constraints. Keep chrome hidden during the gesture and persist the resulting scale/offset. Record actual input events before deciding any implementation is missing. | L | `reader/page.rs`, `reader/state.rs`, `reader/persistence.rs`; gesture and viewport transforms are dependencies |
| Z03 | N/U. A right tap while zoomed turns to the next page at the same zoom and offset. It does not advance across the current page. `pdf-zoom-tap-right.png` and the following menu show page 3 of 45 at 300 percent. | Preserve zoom and pan offset across page turns, clamping only where a different page size requires it. Separate tap-turn input from drag-pan input; compare left taps and physical buttons too. | L | `reader/navigation.rs`, `reader/page.rs`, `reader/state.rs` |
| Z04 | U/C. Nickel PDF menus replace the font tool with pan/zoom near x=740, keep pen/rotation/light/settings/More, and return to Search when opened there. Kaia source uses a general Zoom sheet. | Add the PDF-specific top toolbar and preserve the caller's return context. Keep local fit/region features reachable through a secondary route. Match the bottom page slider and document-title row without showing EPUB line-spacing/alignment controls. | M | `reader/overlay.rs`, `reader/panels.rs`, `reader/mod.rs`, `reader/navigation.rs` |
| Z05 | U. `pdf-zoom-menu.png` shows an overview at `42, 294, 303, 575`, a `253 x 422` page thumbnail, viewport rectangle, page index and 300-percent label. `pdf-menu.png` at reduced scale has no overview. | Add a scale-dependent overview while the zoomed menu is open. Derive the viewport rectangle from actual page/viewport transforms. Capture before choosing the visibility threshold. Do not invent thumbnail dragging from a still image. | L | `reader/overlay.rs`, `reader/page.rs`, `reader/state.rs`; thumbnail rendering is a dependency |
| Z06 | U/C. `pdf-move-tool.png` shows a 138 px-high strip under the toolbar, a y=248 slider, endpoint marks and overview, with the lower menu hidden. Kaia source presents radio choices and buttons in a large sheet. | Build the top zoom strip from y=179 to 317, track x=131 to 1111, 48 px ring thumb, minus near x=68 and plus near x=1195. Keep the overview and expose a verified scale range and step behavior. Hide the lower navigation while this tool is active. | M | `reader/panels.rs`, `reader/overlay.rs`, `reader/commands.rs` |

## Home and alternate home state

Compare Nickel `home.png`, `home-after.png` and `home-two-books.png` with Kaia `home.png` and `home-2.png`. Kaia's second capture changes progress/time and cover height but preserves the hero layout. Nickel's two-book state confirms independent covers across the recent row. Counts and contents differ; the changed Nickel promotional wording is not a layout change.

| Id | Evidence and difference | Concrete change for Kaia | Size | Likely source files |
|---|---|---|---|---|
| H01 | V. Kaia shows a large Kaia heading at `48, 81`, battery/Wi-Fi text and a settings gear. Nickel has a clock at `68, 75` and light, Wi-Fi, battery, sync and search icons. | Replace the large app-name heading with the compact clock/status/action row. Add direct light, Wi-Fi, battery and search actions at the reference positions. Omit Kobo sync rather than binding the same icon to an unrelated network service. Keep settings under More. | M | `home/mod.rs`, `common.rs`, `settings/quick.rs` |
| H02 | V. Kaia puts Continue reading at y=268, then a 200 x 268 px cover at `48, 329` in the first capture, 200 x 300 px in `home-2.png`, with large title, author, long progress bar and time estimate to its right. Nickel uses a 256 x 339 px cover at `80, 237` with only a progress caption below. | Move the current cover right 32 px and up 92 px, increase its size to the reference box, and put the progress caption below at y=611. Remove the heading and adjacent metadata from this primary home card. Keep those details in book details. | S | `home/mod.rs` |
| H03 | V. Kaia leaves most of the middle screen empty and puts a bold library/reading/finished summary near y=1429. Nickel uses two substantial library/group cards starting at x=79 and 644, y=852. | Replace the bottom summary line with two local library/group cards in those columns, with cover stacks above headings near y=1151 and counts near y=1211. Use real book covers and valid groups. Preserve the card layout when the library is small without filling it with junk entries. | M | `home/mod.rs`, `home/controls.rs`, `library/data.rs` |
| H04 | V. Kaia's navigation has four items, starts at y=1529, and marks selection with a green underline near y=1642. Nickel's installation has five items, starts at y=1554, and puts a red 6 px selected mark at the top. | Match the bottom bar's 126 px height, top divider, top selected mark and icon/label placement. Use Kaia's theme accent for the active treatment and keep filled/bold cues. The captured red is Kobo branding, not a Kaia color token. Build the local Home/books/More routes; omit discovery services and external NickelMenu rather than inventing equivalents. Keep notes accessible from More. | M | `common.rs`, `mod.rs`, `home/mod.rs`; shared navigation drawing also changes |
| H05 | V/U. Nickel's lower home cards include store/subscription or saved-article promotions. Kaia has no such blocks. | Exclude those service actions under B-103. Keep the measured local-card layout; do not add Kobo logos, promotional copy or nonworking service buttons to fill the space. Capture the resulting local-only home composition for owner review. | S | `home/mod.rs`, `home/controls.rs` |

The exact number and distribution of Kaia's bottom destinations is a scope exception created by B-103's service exclusions. Match the visual construction and local flows; do not add service functionality merely to force five cells. Record the final local-only arrangement in the next device comparison.

## Home recent row and status popovers

No Kaia capture shows more than one recent Home card or an open status popover. The Home controls themselves are visible, and `settings-display.png` provides related light-setting evidence.

| Id | Evidence and difference | Concrete change for Kaia | Size | Likely source files |
|---|---|---|---|---|
| H09 | V/U. `home-two-books.png` has separate covers at x=80 and 362, both 256 px wide, aligned at y=576, with progress below. Kaia still shows one large hero and metadata. | Extend the primary Home area to independent recent-book tiles at 282 px column pitch. Preserve each cover's aspect ratio and bottom alignment. Capture ordering and overflow with several books. | M | `home/mod.rs`, `home/controls.rs` |
| H10 | U/C. `home-light.png` uses the reader light layout shifted to `363, 139, 803, 869`. Kaia has no open status-light capture. | Share the dedicated light panel's controls and state with the reader, but anchor it under the Home light icon. Match 26 percent/Auto/23:00 for the comparison fixture. Do not reopen general device quick settings in place of this focused panel. | M | `settings/quick.rs`, `home/mod.rs`, `reader/panels.rs` |
| H11 | U. `home-wifi.png` has a `324, 139, 930, 939` callout, compact radio toggle, five network rows at 137 px pitch, signal/lock symbols and a bottom settings action. Kaia's Wi-Fi popup is uncaptured. | Add the anchored radio/network list, reuse real signal/security state and route its settings action to Wi-Fi. Capture off, scan, join, password and failure states. Do not copy private network names from the reference. | M | `settings/quick.rs`, `settings/sections.rs`, `home/mod.rs` |
| H12 | U. `home-battery.png` shows a `585, 139, 634, 239` popup with charge and a power-settings action. Kaia only shows battery text in the captured header. | Make the status battery open the compact popup with a live charge value and direct power route. Do not hardcode the captured percentage. | S | `settings/quick.rs`, `home/mod.rs`, `settings/commands.rs` |

## Global search and results

Nickel references are `home-search.png`, `search-scope.png` and `search-results.png`. Kaia has only the in-book search capture; there is no global search result or scope capture.

| Id | Evidence and difference | Concrete change for Kaia | Size | Likely source files |
|---|---|---|---|---|
| Q04 | U/C. Home search shares the reader query field/keyboard shell but initially targets the store. The scope menu at `823, 222, 380, 474` has five 91 px rows. | Share the search shell across entry points, with local library as the Home default and current book as the reader default. Provide supported library/book/annotations/dictionary scopes and omit the store scope. Use the anchored scope dropdown; show the book selector only when needed. | M | `library/views.rs`, `library/mod.rs`, `reader/panels.rs`, `reader/mod.rs` |
| Q05 | U. The library result state has no keyboard, a result count/sort row, a `124 x 182` cover at `84, 366`, title near `230, 422`, right-hand status and a small centered page indicator near y=1577. | Match the compact result row and hide the keyboard after submission when appropriate. Restore query, scope and results when returning from the opened book. Capture local no-result, multiple-result and sorting states. | M | `library/views.rs`, `library/mod.rs`, `reader/navigation.rs` |

## My books

Compare Nickel `mybooks.png` and `library-books.png` with Kaia `library.png`. Kaia has three valid entries; Nickel includes accidental imports. Compare geometry rather than names, covers or counts.

| Id | Evidence and difference | Concrete change for Kaia | Size | Likely source files |
|---|---|---|---|---|
| B01 | V/C. Kaia has a large Library heading near `48, 98`, five bold tabs at y=243 ordered Books, Folders, Collections, Series, Authors, and a green underline at y=310. Nickel has four tabs ordered books, authors, series, collections. | Match the four primary tabs at y=159, with a thin rule at y=210 and a top-row active underline. Move folders into the library menu. Use the common status row instead of a large library heading. | M | `library/views.rs`, `library/mod.rs`, `library/commands.rs` |
| B02 | V. Kaia puts four large controls at y=338 to 433: All books, Sort: Title, Covers and List. A separate selection icon sits in the header. Nickel shows compact filter, sort and overflow controls. | Put all-book filtering near x=92, recent sorting near x=374, and overflow at x=1157, y=269. Move display-mode and multi-select actions into overflow while preserving them. Capture the menu choices before claiming matching interaction depth. | M | `library/views.rs`, `library/commands.rs` |
| B03 | V. Nickel has four 254 px columns at x=75, 344, 613 and 882, aligned cover bottoms, status captions and individual ellipses. Kaia has three 200 x 300 px covers at x=137, 531 and 925, y=471; large title/author text below; and 12 px progress strips. No per-book ellipsis is visible. Previous/Next buttons measure 321 x 107 px at y=1373. | Align four columns and the measured row structure. Replace prominent grid title/author blocks with smaller status captions. Use progress/read-state captions and individual overflow actions below covers. Do not enforce square covers from Nickel's junk entries. Replace the large bottom pager with the measured right scrollbar and verify scrolling using a larger valid library. | M | `library/views.rs`, `library/press.rs`, `library/details.rs` |

## Author library

Nickel reference is `library-author-unknown.png`. Kaia has no author-group capture; `library/views.rs` has `author_list` and `author_books` builders.

| Id | Evidence and difference | Concrete change for Kaia | Size | Likely source files |
|---|---|---|---|---|
| B04 | U/C. Nickel replaces category tabs with a back arrow, author heading and right action, retaining filter/sort and the book grid. | Capture Kaia's author view. Match the heading row near y=152, rule at y=198, filter/sort below and four-column grid. Return to the caller with Back. Keep local same-author navigation and omit a store-backed action if it has no local meaning. | M | `library/views.rs`, `library/mod.rs`, `library/data.rs` |

## Library dropdowns, list and book details

None of these Kaia popup/detail/list states is captured. Existing builders and commands should be reused; the source's presence alone is not visual or behavior parity.

| Id | Evidence and difference | Concrete change for Kaia | Size | Likely source files |
|---|---|---|---|---|
| B05 | U/C. `library-filter.png` is a square `67, 327, 429, 565` popup with six status/source choices and counts. Kaia's grid shows a large All books button but not its open state. | Replace the large filter presentation with the anchored six-row layout, check mark and counts. Keep only meaningful local import/download states. Match 91 px row pitch and black active trigger. | M | `library/views.rs`, `library/commands.rs`, `library/data.rs` |
| B06 | U/C. `library-sort.png` has seven 91 px rows at `349, 327, 445, 656`: recent, added, title, author, series, size and type. Kaia's source has different sort choices including progress; its capture shows title sorting. | Add file size/type where metadata supports them, order primary choices to the reference and put Kaia-only sorts in a secondary group if retained. Default the comparison fixture to recent. Add checked state and the square popup geometry. | M | `library/views.rs`, `library/commands.rs`, `library/data.rs` |
| B07 | U/C. `library-view-options.png` and `library-list-menu.png` show the same anchored menu near `746, 308, 451, 474`; its first action switches list/covers. Kaia exposes two large display buttons and a header selection icon. | Move display-mode switching and book management into overflow. Include download management only for supported local/network features, with real enabled states. Do not copy store download actions as dead controls. Preserve selection capability behind management. | M | `library/views.rs`, `library/commands.rs`, `library/mod.rs` |
| B08 | U/C. `library-list.png` fits five book rows at about 243 px pitch, with `124 x 164` covers, title/author at x=232, status right and per-book overflow. Kaia list rendering is uncaptured. | Match those five-row metrics, 1 px rules and compact text hierarchy, with the first row beginning near y=363. Keep long titles/authors within their column and preserve right-hand status/overflow. Capture with varied real cover shapes. | M | `library/views.rs`, `library/press.rs` |
| B09 | U/C. `book-menu.png` shows six actions in `144, 592, 460, 565`: read, details, collection, unread, finished and remove. Kaia's per-book action state is uncaptured. | Anchor each book's action menu to its ellipsis and provide the six local actions at 91 px pitch with thin dividers. Preserve current safeguards and confirm removal scope in the subsequent dialog. | M | `library/press.rs`, `library/commands.rs`, `library/details.rs` |
| B10 | U/C. `book-details.png` has an inset dialog, `285 x 377` cover at `81, 140`, title/author at x=392, Read and overflow beside it, then summary/details tabs near y=811. Kaia details exist in source but are uncaptured. | Rebuild the details shell and upper metadata/action grouping to these positions. Use Kaia's own dialog title and open icons; flow local descriptions below the y=861 tab rule at about 45 px pitch. Preserve title wrapping and aspect ratio. | M | `library/details.rs`, `library/mod.rs`, `library/commands.rs` |
| B11 | U/C. `book-details-tab.png` shows six metadata lines from y=898, 45 px apart: identifier, language, filename, path, size and format. | Add the compact details tab with actual local metadata and sensible truncation/scrolling for paths. Do not copy the fixture's UUID, filename or device path into code. Capture missing metadata and long path cases. | M | `library/details.rs`, `library/data.rs` |
| B12 | U/C. `book-details-more.png` has a four-row popup at `455, 371, 460, 383`: unread, finished, collection and remove. | Reuse book commands in an overflow anchored beside Read, with 91 px row pitch. Keep detail state and caller context after dismissing the popup or applying a status change. | S | `library/details.rs`, `library/commands.rs` |

## Author, series and collection tabs

Kaia has source builders but no capture for these selected tabs or their empty states.

| Id | Evidence and difference | Concrete change for Kaia | Size | Likely source files |
|---|---|---|---|---|
| B13 | U/C. `library-authors.png` is a compact surname-sorted list with 120 px pitch, names at x=99 and counts at x=1174. | Match the author list and sort row, preserving unknown-author handling without treating junk counts as fixtures. Open the author book-grid route in B04 and return to the same author-list position. | M | `library/views.rs`, `library/data.rs`, `library/mod.rs` |
| B14 | U/C. `library-series.png` has a centered empty-state illustration in a 504 px circle, heading near y=987, two explanation lines and a store action. | Use original open artwork or a Kaia-drawn illustration at the same size, with a short local-series explanation. Omit the store action and keep local discovery meaningful. Capture populated and empty series lists. | M | `library/views.rs`, `library/data.rs` |
| B15 | U/C. `library-collections.png` places its illustration at `380, 348, 504, 504`, heading near y=921, explanation beneath and a `359 x 80` create action at `453, 1268`. | Match the local collection empty state, original artwork and create button. Preserve the creation/name validation flow and capture its keyboard, cancel and successful result. | M | `library/views.rs`, `library/commands.rs`, `library/mod.rs` |

## More tab and additional More capture

Nickel references are `more-tab.png` and the temporary `x.png` inspected before removal. They show the same page at different clock times. Kaia's home has direct Notes and Settings tabs and no More tab; no Kaia More capture exists.

| Id | Evidence and difference | Concrete change for Kaia | Size | Likely source files |
|---|---|---|---|---|
| H06 | V/C/U. Kaia has no captured equivalent to Nickel's secondary navigation list. | Add a local More destination with the title near `89, 145`, rows at 134 px pitch, icons near x=85, labels near x=172 and rules from x=67 to 1196. Route supported local favorites, notes, activity, experiments, settings and help through it. Keep notebook creation distinct from book annotations; the captured Nickel wishlist is store-backed. Use Kaia's open icons and about 25 px italic or metrically matched label caps. | M | `mod.rs`, `common.rs`, `settings/mod.rs`, `notes/mod.rs` |
| H07 | V. Nickel's More list includes Dropbox, Drive and service-linked saved articles. Kaia's scope excludes those integrations. | Omit those service rows and logos. Preserve the order and row construction of supported local destinations. Do not leave dead actions or present a local feature under a service's name. Capture the local-only More page after implementation. | S | `mod.rs`, `settings/mod.rs` |

## Favorites, notebooks and beta features

Nickel `more-favorites.png` is a store wishlist; `more-notebooks.png` is a stylus introduction; `more-activity.png` actually shows the notebook list after that introduction. It provides no statistics reference. Kaia `notes.png` holds book annotations, not freeform notebooks, and `settings-stats.png` shows local reading statistics.

| Id | Evidence and difference | Concrete change for Kaia | Size | Likely source files |
|---|---|---|---|---|
| H13 | U/C. The reference wishlist has a back/title row, filter, centered illustration and store call to action. Kaia has local favorites in source, but no capture. | Retain local favorites under a clear local label. Reuse the compact header/filter and empty-state spacing if appropriate; omit store wording/action. Capture local favorite books before deciding card differences. | M | `library/views.rs`, `library/mod.rs` |
| H14 | U. The notebook introduction is an `800 x 1100` dialog near `232, 290`, with stylus artwork, three headline lines, body text and two actions. No Kaia freeform notebook introduction is captured. | If notebook support is in the implementation scope, use original artwork and concise local guidance in this dialog geometry. Do not copy Kobo's hardware/store activation requirement. Keep book notes reachable without a notebook introduction. | M | `notes/mod.rs`, `reader/ink.rs`, `mod.rs`; a separate notebook surface may be required |
| H15 | U/C. The actual notebook list in `more-activity.png` has a heading near `165, 163`, sort below, and a dashed `192 x 259` new-notebook tile at `109, 373`. Kaia's captured Notes page has annotation type filters instead. | Keep the two data types separate. Add a dedicated notebook route and new-notebook tile only with real notebook creation/storage support; do not relabel annotation filters as notebooks. Capture a true Nickel activity screen before changing Kaia statistics to match it. | L | `notes/mod.rs`, `notes/data.rs`, `mod.rs`; notebook data and canvas work are dependencies |
| H16 | V/C/U. `more-beta.png` has the inset settings shell, browser launch and off controls for large print and saved words. Kaia `settings-display.png` places Large print at the bottom of the light page; experiments exist separately in source. | Put optional experiments in a compact dialog with flat strips and off square checks. Move the large-print entry to the matching experiments route or retain a cross-link for accessibility. Provide browser/saved-word actions only where supported, preserving network opt-in. | M | `settings/experiments.rs`, `settings/sections.rs`, `settings/section.rs`, `reader/panels.rs` |

Implementation evidence, 2026-10-15. Headless tests in `rust/kaia-app/tests/library_round4.rs` cover Q04/Q05 with local books, current-book text, annotation text and dictionary lookup. Global search shares the reader's field and keyboard controls, retains its query and results after opening a book, and offers relevance and title sorting. Current-book results use stable CREngine positions for reflowable documents. Search covers and result paging use the inset dialog layout. The related images are `global_search*.png` under `rust/kaia-app/tests/golden/`.

H11 has off, scanning, network list, masked password, joining, failure, retry and connected states. The `wifi_*.png` fixtures contain no network names. The platform scan and connection methods are synchronous; device responsiveness during those calls and real-radio recovery still need testing. The settings action keeps the existing Network route. W01's settings reorganization remains separate.

H13 has local empty and populated Favorites views. Favorites filters do not change the main library's saved filters. H14/H15 have a separate informational notebook route, with no creation action because this build has no notebook storage. H16 uses the gray inset dialog, section strips, the existing experiment policy and a link to Large print settings. No browser or store actions were added. See `favorites_*.png`, `notebooks_*.png`, `experiments.png` and `home_empty.png`.

B10/B12 and the other library dialogs now have an opaque gray surround. Rating, Tags, Statistics and Favorite are in the details ellipsis menu; Synopsis and Details retain the text area below the tabs. `library_details.png`, `library_metadata.png` and `library_details_actions.png` show these states. These are headless captures, not device gesture or refresh-parity evidence.

## NickelMenu

Nickel reference is `nm-menu.png`. This is the external launcher the owner uses to start both Kaia apps. It is not an internal Rust app screen, and no internal Kaia counterpart is expected.

| Id | Evidence and difference | Concrete change for Kaia | Size | Likely source files |
|---|---|---|---|---|
| H08 | V/U. The external menu launches Lua/Rust and manages remote access/screenshots. Its 445 x 474 px popover is not a missing Kaia settings screen. | No app change is required for visual parity here. Retain the external launch route in the device test procedure and capture Kaia immediately after launch. If startup returns to the wrong page, inspect the Home route rather than embedding NickelMenu. | S, verification only | `home/mod.rs`, `mod.rs` |

## Settings index

Compare Nickel `settings.png` with Kaia `settings.png` and `settings-page2.png`. Kaia spreads eleven categories across two pages, six then five, with Search settings and bottom app navigation. Nickel fits eleven rows in its dialog.

| Id | Evidence and difference | Concrete change for Kaia | Size | Likely source files |
|---|---|---|---|---|
| S08 | V. Kaia uses a full app page: large heading near `48, 62`, a 371 x 94 px search button at y=151, six then five categories at 166 px pitch, large chevrons, a 107 px-high pager and bottom navigation. Nickel fits eleven rows in an inset dialog. | Put the settings index inside `25, 25, 1214, 1630` with a dark-gray surround. Use the heading near `124, 205`, 107 px row pitch, 1 px rules and roughly 30 px italic caps. Keep search as a secondary action if retained. | M | `settings/mod.rs`, `settings/section.rs`, `settings/entries.rs` |
| S09 | V/C. Kaia categories include Home, Reading, Display and light, Network and sync, Device, Data and Advanced. Nickel groups language with dictionaries, date with time, and power with privacy. | Reorganize local settings into those reference groupings while preserving Kaia-only settings behind suitable secondary rows. Keep account/DRM/cloud categories out. Use a back arrow on subpages and close to return to the original caller. | M | `settings/commands.rs`, `settings/sections.rs`, `settings/mod.rs`, `settings/section.rs` |
| S10 | V. All eight captured Kaia settings subpages use a bold heading at `144, 98`, full white background and a large bottom pager at y=1509 to 1616, even on one-page sections. Main labels have about 38 px caps, secondary labels about 31 px, rows often 166 px apart, toggles 160 x 53 px and button outlines 6 px with rounded corners. Nickel uses smaller text, thin rules, flat strips, right-aligned controls and square checkboxes. | Add the reference settings-row treatment instead of reusing the large reader-sheet controls unchanged. Match 38 px checkbox ink, 49 px section strips, 24 to 25 px label caps and about 43 px explanatory pitch. Keep touch targets larger than the ink. | M | `settings/entries.rs`, `settings/section.rs` |

## Power settings page one

Nickel reference is `settings-power.png`. Kaia `settings-device.png` is page one of five: a large Storage button, four Standby radio choices occupying y=423 to 797, explanatory text, and a Sleep after heading at y=976 whose value is on a later page. It does not reveal the sleep timeout or actual sleep screen.

| Id | Evidence and difference | Concrete change for Kaia | Size | Likely source files |
|---|---|---|---|---|
| P01 | C/U. Kaia's `DEFAULT_SLEEP_MINUTES` is 15; Nickel shows 5. Kaia already defaults shutdown to zero/never. | Change the fresh sleep default to 5 minutes. Retain never for shutdown and present both as compact right-aligned dropdown rows. Preserve saved timeout choices. | S | `settings/keys.rs`, `settings/sections.rs` |
| P02 | C/U. Kaia defaults the sleep-screen choice to cover, with details as another choice. Nickel has separate checked controls for current book, full-screen cover and details. | Expose those three independent preferences and default them on for the reference configuration. Add the details panel to the full-screen cover path. Capture an actual sleep screen before specifying its panel position or crop. | M | `settings/sections.rs`, `settings/keys.rs`, `settings/sleep.rs` |
| P03 | C/U. Kaia gates page-button wake on platform support and defaults it off; Nickel's checkbox is on. Kaia already has a sleep-cover control. | Keep the capability check, investigate and test page-button wake support, and match the on default when it is supported. Put wake, charging indicator and cover sleep/wake in the same power page. Verify the real cover sensor and buttons before enabling unsupported hardware behavior. | L | `settings/sections.rs`, `settings/keys.rs`, `settings/sleep.rs`; platform support is a dependency |

## Power settings page two

Nickel reference is `settings-power-2.png`. Kaia has no PIN/privacy capture; the source has a screen-lock section and a PIN route.

| Id | Evidence and difference | Concrete change for Kaia | Size | Likely source files |
|---|---|---|---|---|
| P04 | C/U. Nickel presents an unchecked PIN row and a disabled change-PIN row beneath a gray strip. Kaia's resulting screen is not captured. | Group PIN enable/change under power/privacy, match the compact two-row structure, and gray out change-PIN while lock is off. Keep Kaia's existing PIN workflow, then capture enable, change, wake and failure states to compare. | M | `settings/sections.rs`, `settings/section.rs`, `pin/mod.rs` |
| P05 | V. Nickel has checked usage-reporting and store-history options. These conflict with Kaia's no-telemetry rule or excluded services. | Omit both service-specific rows from Kaia. Do not adopt their captured on states as product defaults. Use the remaining space for supported privacy controls only. | S | `settings/sections.rs` |

## Wi-Fi settings

Nickel reference is `settings-wifi.png`. Kaia `settings-network.png` shows page one of four for network services, but no Wi-Fi radio, scan or credential state. Kaia source currently places a radio toggle in quick settings and networking functions in its Network and sync section.

| Id | Evidence and difference | Concrete change for Kaia | Size | Likely source files |
|---|---|---|---|---|
| W01 | V/C/U. Nickel groups Wi-Fi radio, connect and forget actions on one page. Kaia `settings-network.png` instead shows service setup: OPDS, Calibre and document/progress sync. No captured radio, scan or known-network state is available. | Add or reorganize a dedicated Wi-Fi subpage with strips at y=280 and 579, a checked radio row near y=385, and connect/remove rows around y=687 and 800. Keep it reachable from status and settings. Capture radio-off, network scan, join and forget states. | M | `settings/sections.rs`, `settings/section.rs`, `settings/quick.rs` |

## Language and dictionaries

Nickel reference is `settings-language.png`. No Kaia language/dictionary-management capture exists. Kaia source places language under Display and light and dictionary management under Reading.

| Id | Evidence and difference | Concrete change for Kaia | Size | Likely source files |
|---|---|---|---|---|
| W02 | C/U. Related language and dictionary controls are split across Kaia categories. Nickel places them on one sparse page. | Group the current language near y=375, dictionary count/Edit near y=526 and installed size near y=626, with gray strips at y=280 and 430. Retain Kaia's approved local/import/download choices in the Edit route. Do not set the fixture's two dictionaries or 13.3 MB as defaults. | M | `settings/sections.rs`, `settings/section.rs`, `reader/dict.rs`, `reader/panels.rs` |
| W03 | V/U. Nickel UI is Spanish; the Kaia captures are English. It is unknown whether Kaia was explicitly set to English or failed to use the desired locale. | Capture Kaia with Spanish selected and check truncation, navigation labels, dropdown values and keyboard choice. Preserve localized short labels and Kaia-written explanations. Do not report English itself as a defect without checking the selected language. | M | `settings/section.rs`, `settings/sections.rs`, `reader/panels.rs`, `library/views.rs` |

## Date and time

Nickel reference is `settings-datetime.png`. No Kaia date/time capture exists. Kaia source exposes time zone and opt-in network time under Device.

| Id | Evidence and difference | Concrete change for Kaia | Size | Likely source files |
|---|---|---|---|---|
| W04 | C/U. Kaia's time controls are not grouped as Nickel's four-row date/time page. | Add a date/time subpage with rows bounded by y=280, 393, 506, 619 and 732. Show date, time, zone and a 24-hour switch near `1073, 652, 88, 49`. Keep manual editing and Kaia's opt-in NTP policy explicit. Capture all editors and verify time-zone changes. | M | `settings/sections.rs`, `settings/section.rs`, `settings/commands.rs` |

## Device information

Nickel reference is `settings-device-info.png`. Kaia `settings-about.png` and `settings-device.png` show separate app information and power controls. Hardware facts remain uncaptured. Source also has Storage and Data routes.

| Id | Evidence and difference | Concrete change for Kaia | Size | Likely source files |
|---|---|---|---|---|
| W05 | V/C/U. Nickel groups hardware facts, storage and software version in one two-column page. Kaia `settings-about.png` shows app/version, license, source and credits, while `settings-device.png` shows power controls. Hardware/storage facts are not visible in these captures. | Group current hardware/network/storage facts under the first strip at y=280 and Kaia's build/version under the second at y=748. Right-align values and use about 103 px hardware row pitch. Read actual device values; do not copy the owner's serial, MAC or IP into code or docs. | M | `settings/sections.rs`, `settings/facts.rs`, `settings/storage.rs` |
| W06 | V/C/U. Nickel's maintenance area includes account repair and whole-device factory reset. Kaia's decisions require its own updates and two scoped reset actions. | Match the section/row layout, but expose only Kaia's signed-update path, reset settings, erase Kaia data, and Kaia license/build information. Keep typed confirmation for the scoped reset actions. Do not add Kobo account repair or firmware factory reset. Capture those confirmations separately. | M | `settings/sections.rs`, `settings/reset.rs`, `settings/section.rs` |

## Additional captured settings differences

These Kaia captures have no exact Nickel counterpart beyond the shared settings construction. Preserve their local features without inventing a one-to-one content reference.

| Id | Evidence and difference | Concrete change for Kaia | Size | Likely source files |
|---|---|---|---|---|
| S11 | V. `settings-home.png` uses five 166 px rows with 160 x 53 px toggles, explanatory text at y=1158 and a disabled bottom pager. No Nickel Home-customization page was supplied. | Preserve Home section choices behind an appropriate secondary route; apply the compact settings shell and controls, and omit the pager when there is only one page. Reconcile choices with the recent-row/card layout rather than leaving controls tied to removed Home blocks. | M | `settings/sections.rs`, `settings/section.rs`, `home/mod.rs` |
| S12 | V. `settings-library.png` shows a folder path, a `1156 x 94` Scan now button and one finished-books switch, leaving most of the page blank. Nickel's library controls live mainly in the library dropdowns. | Keep folder/import management as a local secondary feature, with compact rows and a modest action button. Move routine read-state filtering to the library filter. Remove one-page pagination and show actual scan progress/result when invoked. | M | `settings/sections.rs`, `settings/section.rs`, `library/commands.rs` |
| S13 | V. `settings-network.png` has an Edit fields button, service rows and three wide switches. Two long labels run into the switches at x=1024. No equivalent service settings are in the Nickel references. | Keep approved OPDS/Calibre/sync settings separate from Wi-Fi. Reserve a fixed control column and wrap or shorten labels before x=1000; use concise descriptions below where needed. Keep all network actions opt-in. | S | `settings/entries.rs`, `settings/sections.rs`, `settings/section.rs` |
| S14 | V. `settings-reading.png` shows both 47 px and 22 KOReader units, plus file-install instructions, in the primary reading-preferences page. Nickel keeps font controls in the compact reader panel and has no engine-unit labels. | Show a single user-facing size representation where needed; keep internal units in diagnostics. Move font import guidance into font management and make normal reading preferences about progress, buttons and gestures. | M | `settings/sections.rs`, `reader/panels.rs`, `settings/entries.rs` |
| S15 | V. `settings-device.png` consumes most of page one with expanded Standby choices, leaving Sleep after as a heading without its value. Nickel shows sleep and power-off values together near the top of one page. | Collapse standby into a secondary dropdown and put Sleep after/Power off first. Show each label with its current value on the same row. Use scrolling/compact rows without orphaning a section title at a page boundary. | M | `settings/sections.rs`, `settings/section.rs`, `settings/entries.rs` |
| S16 | V/U. `settings-stats.png` shows three reading-time totals, a seven-day green chart and finished-book count, with a second page. There is no Nickel activity screenshot despite the `more-activity.png` name. | Keep the working local statistics. Apply shared type/spacing only where supported by other screens, then capture Nickel activity before specifying a statistics redesign. Do not replace it with notebook layout. | S, verification first | `settings/sections.rs`, `library/book_stats.rs`, `settings/section.rs` |
| S17 | V. `settings-about.png` uses large app/version/license/source rows and a long credits block, followed by a disabled pager. Nickel's device-info page separates hardware/software/maintenance. | Put Kaia version and build in the software group and keep required license/credits in their own reachable page. Remove the disabled pager on a one-page About view and apply the inset dialog typography. Verify Rust-specific build/credit wording against the actual shipped components. | M | `settings/sections.rs`, `settings/facts.rs`, `settings/section.rs` |

## Required next device captures

This research did not run the app, press controls or test gestures. No Cargo command was run and no Rust file was edited. The next pass should retain both apps' screenshots outside the repository and record actions in Kaia's words.

1. Capture the same EPUB passage and title page in both apps, using fresh defaults and then saved custom layouts. Include header, footer, progress strip and Adobe numbering. The current `reader-open.png` files are different locations.
2. Recapture the light panel. The existing Kaia `light.png` missed it. Record Auto, bedtime, brightness, a manual warmth change and the return to reading.
3. Capture Kaia's font list, Advanced preview, remaining reading-settings pages, reader More, per-book annotations, selection handles/note entry, installed-dictionary results and search results/history. The new search, selection and no-dictionary captures are baselines. `reader-notes-panel.png` missed its panel.
4. Capture Kaia's library dropdowns, list, book menus/details, author/series/collection groups, More, status popovers, remaining power pages, actual sleep screen, PIN, Wi-Fi, language/dictionaries, date/time and hardware facts. Capture PDF fit, pinch, pan, turn-at-zoom, overview and slider using the same document as Nickel.
5. Check transitions as well as stills: center-menu taps, outside taps, taps versus swipes, pinch, brightness edge drags, long press and selection handles, page-button direction in each orientation, chapter refresh and return from settings. Record actual results before marking behavior gaps closed.
6. Use valid books for the library pass. Ignore the accidental imports and compare card geometry with varied real cover aspect ratios. The external NickelMenu capture is launcher evidence, not a missing internal screen.

## Ten changes with the most reading impact

1. Calibrate default body type and line pitch to the measured 57 px caps, 41 px x-height and 140 px pitch, R01 and R02.
2. Restore the centered chapter header, title/progress footer and thin page-edge progress strip, R04 to R06.
3. Resolve rendered-page versus Adobe-page semantics at matching book locations, R03, R07 and R09.
4. Rebuild the reader toolbar and lower navigation around Nickel's tool order, spacing and compact seek control, M01 to M08.
5. Replace the full-screen typography sheet with the 562 px top panel, sliders and alignment segments, F01 to F05.
6. Add the font dropdown and before/after Advanced workflow, F06 to F09.
7. Expand Contents to the inset dialog and restore its 16-row density and unified Annotations tab, C01 to C06.
8. Group the three reading-settings pages and verify every default gesture and button action, S01 to S07.
9. Give reading its own anchored light panel with Auto and the 23:00 night setting, L01 to L03.
10. Match the word-selection/dictionary and search entry flows while preserving local data and network opt-in, D01 to D04 and Q01 to Q03.
