# Nickel screen reference

Recorded on 2026-10-14 for B-103. The inventory contains 63 Nickel captures from a Kobo Libra Colour; the owner identified the reading and settings states as defaults. Every PNG in `/Users/wildeax/kaia-re/analysis/nickel-screens/` was opened and inspected. The temporary `x.png` was also inspected before its removal, so this document retains 64 screen sections. The directory grew during review to include Home, library, search, notebook and PDF states. The 24 Rust captures are compared in [GAPS.md](GAPS.md).

The device-info capture reports firmware **4.42.23303**. Section 1 of [PARITY.md](PARITY.md) describes a separate study of 4.45.23697. Do not treat their versions or evidence as interchangeable. The device clock displays October 15 in UTC, which is consistent with the owner's October 14 session in Colombia.

B-103 makes Nickel's layout, flows, gestures and behavior the reference for the Rust app. It supersedes conflicting visual choices in the October 13 additions to [DESIGN.md](../specs/DESIGN.md), including the KOReader-derived reading defaults and icon-over-label reader bars. Kaia retains its own licensed fonts and open icons. These notes describe visible geometry and behavior in our own words. No screenshots, extracted artwork, font files, logos, dictionary text or long Nickel UI strings belong in the repository.

## Measurement and evidence

All coordinates are native screenshot pixels on a 1264 x 1680 portrait panel. The origin is the upper left. A box is written as `x, y, width, height`. At 300 PPI, 1 mm is about 11.81 px. Layout coordinates read from the images are approximate, normally within 4 px. Thin rules, sampled colors and the reading text bands were also checked by decoding the PNG pixels with Python's standard library. No image was copied or changed. Visible glyph bounds are not font point sizes, CSS sizes or touch-target bounds. Font estimates below describe cap height, or x-height where stated. Allow about 2 px for antialiasing thresholds and glyph choice.

Each screen section lists all visible controls. Their purpose follows the visible affordance and the owner's brief. A still image cannot establish hitboxes, drag thresholds, timing, persistence, focus order, outside-tap dismissal or the result of pressing a control. Entry and exit routes below are the expected routes to verify on the next device pass unless two captures show the related states. Gestures explicitly enabled in settings are visible configuration evidence, not a gesture test. Uncaptured menus and destinations remain unmeasured.

The external device-sweep `NOTES.md` provides a second kind of evidence: reported actions and results. It records left/back, center/menu and right/forward tap zones; top-right bookmark toggling; pinch zoom around the fingers; drag-to-pan; and right-tap PDF page turns that preserve zoom and offset. It also reports about 60 ms injected touches for icons and 180 ms for list/menu actions. These are successful remote-input durations, not measured minimum thresholds or prescribed user hold times. Where a section says a gesture still needs verification, that means thresholds, edge cases or Kaia parity remain untested. The notes describe the default prose as justified by book CSS without hyphenation; the screenshot itself cannot establish a global hyphenation setting.

The library includes roughly 120 files accidentally imported from Kaia's program folder. Ignore their names, covers, status counts and author grouping as sample content. Keep the grid, card alignment, controls and spacing. The external NickelMenu entry and screenshot toggle also reflect this owner's installation, not a stock Nickel feature requirement. Kobo store, account sync, OverDrive, cloud links and DRM books stay in Nickel. Do not create fake Kaia controls for those services.

### Shared geometry and colors

- The normal application status row has a clock at about `68, 75`, with 21 px caps. The light, Wi-Fi, battery, sync and search icons have centers near x=765, 864, 960, 1059 and 1157, y=85. Their ink boxes are about 44 to 55 px wide and 40 to 50 px high. This row occupies roughly y=40 to 123. Each icon opens its named status or action view. Sync is a Kobo service, outside Kaia's scope.
- The normal bottom navigation starts with a 1 px gray rule at y=1554 and fills the remaining 126 px. Five equal cells in this installation are about 253 px wide. Icon ink sits around y=1575 to 1618 and label ink around y=1632 to 1656, with 24 px caps. The active cell has a 6 px top mark at y=1554 to 1559, a filled or heavier icon, and a bold label. Cells are Home, books, discovery, More and the installed NickelMenu. The last two service-specific destinations are not a mandate to duplicate their services in Kaia.
- Reader toolbar screens use a different status row. Clock ink starts at `50, 34`, 20 px caps. Battery ink is around `1171, 29, 55, 28`. A small 50 x 1 px mark sits near `615, 44`; its purpose is not established. The action row occupies y=88 to 178. A 1 px black line at y=178 separates it from the page.
- The large settings and contents dialog uses `25, 25, 1214, 1630`, a 1 px black border and square corners. Its outer surround is solid `#4d4d4d`. The centered dialog caption has about 23 px caps at y=71 to 94. The close cross occupies roughly `1167, 65, 32, 32`. Settings subpages also have a back arrow near `62, 65, 36, 32`. The main heading begins at x=124, y=202 to 244, with about 39 px caps. The content edge is x=100, labels start at x=125, and the right content edge is x=1156 or 1164. No application status row or bottom navigation is visible in this dialog.
- Settings section strips are about 49 px high, `#d9d9d9`, with a 1 px black top rule. Their sans-serif caps are about 20 px. Setting labels and values use serif text with about 24 to 25 px caps and 17 to 19 px x-height. Values and text actions are often italic. Explanatory text has about 43 px line pitch. Standard checkbox ink is 38 x 38 px, with a 3 px border. The word beside it is an enable-action label even when the box is empty; the check mark establishes the state. Reading/power pages have a thin vertical scroll track at x=1197 from y=298 to 1604 and an outlined 12 px-wide thumb.
- Main ink is `#000000` on `#ffffff`. Settings backgrounds and unselected font segments use `#d9d9d9`; the selected alignment segment uses `#262626` with white text. Contents row rules are `#aaaaaa`; many other rules are black or medium gray and 1 px thick. The active navigation and contents-tab accent samples as `#bf0000`. This records the target color treatment as well as its non-color state cues. Text antialiasing adds intermediate grays and does not imply extra design colors. Covers supply their own colors.

### Captured default settings

These are the owner's reference defaults or captured settings, not proof of a factory reset. Brightness, language, clock, installed dictionaries and book layout can depend on the device or book.

| Setting | Reference state | Evidence |
|---|---|---|
| Sleep timeout | 5 minutes | `settings-power.png` |
| Power-off timeout | Never | `settings-power.png` |
| Sleep screen | Current book cover on, full-screen cover on, details panel on | `settings-power.png` |
| Page-button wake | On | `settings-power.png` |
| Charging indicator and cover sleep/wake | Both on | `settings-power.png` |
| PIN lock | Off; change-PIN action disabled | `settings-power-2.png` |
| Reader header | Pages left in chapter | `reader-settings.png` and `settings-reading.png` |
| Reader footer | Pages left in book | Same two captures |
| Book progress bar | On | Same two captures and reading pages |
| Page-button direction | Default; upper button back, lower button forward in the portrait diagram | Same two captures |
| Full refresh | Every chapter | `reader-settings-2.png` |
| Reading orientation | Automatic rotation | `reader-settings-2.png` |
| Adobe EPUB page numbers | On | `reader-settings-2.png`; marginal number on title page |
| Dark mode | Off | Empty box in `reader-settings-2.png` |
| Rainbow reduction | Off | Empty box in `reader-settings-2.png` |
| Tap zones | First preset; left back, center menu, right forward | `reader-settings-3.png` |
| Pinch to change font size | On | `reader-settings-3.png` |
| Left-edge swipe for brightness | On | `reader-settings-3.png` |
| Page-turn gesture | Tap or swipe | `reader-settings-3.png` |
| Natural light | Auto on; night time 23:00 | `reader-light.png` |
| Brightness | 26 percent | `reader-light.png` |
| Font selection and alignment | Book/default font; alignment override off | `reader-font-settings.png` |
| Wi-Fi | On | `settings-wifi.png` |
| Language and time format | Spanish; 24-hour clock on | `settings-language.png`, `settings-datetime.png` |

The reading status labels name remaining-page settings, while the supplied EPUB displays a chapter fraction of 1 of 2 and a book fraction of 3 of 96. Preserve both observations. Do not derive a remaining-page formula or equate Adobe pages with rendered screen turns from these stills.

## Reader default body, `reader-default-body.png`

Purpose. Read ordinary book prose with the menus hidden.

Layout. There is no device status bar or title toolbar. The centered chapter/status line has ink at y=50 to 69, 20 px caps, centered on x=632. The prose ink spans about x=143 to 1121, leaving about 143 px on each side. Nine text lines begin at y=226, 366, 505, 645, 785, 926, 1065, 1205 and 1346. Baseline pitch is about 140 px. The last ink reaches y=1419. Sample capital height is 57 px and lowercase x-height is 41 px; descenders make some complete line ink bands 74 to 75 px high. The header-to-body ink gap is about 156 px. The visible body-to-footer ink gap is about 192 px.

The nine lines contain 5, 4, 3, 5, 2, 5, 2, 6 and 5 whitespace-delimited words, about 4.1 words per line. A hyphenated place name counts as one word. These counts describe this passage only. Both text edges align and the word spaces stretch, so the prose is justified. The font panel has alignment override off, which permits book styling to supply this alignment. The image does not establish whether the 143 px inset comes from reader margins, EPUB styling or both.

The centered footer contains the book title and a 3-of-96 page fraction at y=1612 to 1631, also 20 px caps. A full-width 1 px black progress track lies at y=1663. Its filled start is x=0 to 38 and 5 px high, y=1661 to 1665. White space continues to the bottom edge. There are no visible buttons, row rules, tab bar or sliders.

Controls and flow. Open a book from Home or the library, then turn to this passage. The enabled default gestures are a left-zone tap for back, center tap for the reader menu, right-zone tap for forward, horizontal swipe for page turns, pinch for size and left-edge vertical swipe for brightness. Hold a word to reach the selection state. The physical page buttons follow the default direction. The sweep notes report that the top-right corner toggles a bookmark. Exact zone widths, hold duration, swipe thresholds and gesture conflicts still need a device pass. Exit through the menu's Home action.

## Reader default title page, `reader-default-titlepage.png`

Purpose. Display the EPUB's own title page within the same reading layout.

Layout. The shared reading header shows the current section at y=50 to 69. Publisher-supplied book artwork occupies roughly `147, 214, 179, 62`; it is book content, not Kaia chrome. The centered book title occupies about x=205 to 1057, y=551 to 628, with about 71 px caps. Author ink is at y=781 to 843, with about 46 px caps. Publication metadata starts near `143, 1096`; subsequent lines begin near y=1214 and 1336. Their cap heights are about 46 to 49 px and line pitch about 120 px. A marginal Adobe page marker sits near `1184, 600`, about 23 px high. The footer has the book title and a 2-of-96 fraction at y=1612 to 1631; the 5 px progress fill is about 26 px long on the y=1663 track. There are no app buttons or tab bars.

Controls and flow. This is document content, so preserve book styles and use the same reading gestures and menu exit as the default body page. Do not turn the publisher artwork, title-page font sizes or metadata into an app template. Compare this page with the same EPUB location in Kaia, not Kaia's cover image.

## Open reader, `reader-open.png`

Purpose. Show another prose position with menus hidden.

Layout. Header, footer, palette, progress track and approximately 143 px side insets match the default body capture. Nine prose lines start near y=225, 366, 506, 646, 785, 926, 1065, 1205 and 1346. Pitch is again about 140 px and text size is the same. This passage has about 2 to 6 words per line. The footer still displays 3 of 96 despite the changed passage. That is evidence against assuming each screen turn increments this EPUB page number. Paragraph-ending lines have natural spacing; full lines stretch to both edges.

Controls and flow. The default-body controls and gestures apply. This is the underlying page visible in `reader-menu.png`; use that pair to judge menu occlusion without confusing it with a font-size change. No extra controls appear in the reading state.

## Reader menu, `reader-menu.png`

Purpose. Expose reading tools and navigation while keeping the current page visible.

Layout. Use the shared reader status row. The back arrow is around `34, 112, 37, 32`; its bold Home-return label begins at x=107, y=116, with about 25 px caps. Tool centers at y=128 are x=750 for pen mode, 842 for rotation, 932 for light, 1018 for font, 1104 for settings and 1199 for More. Most tool ink is 44 to 52 px wide. The pen control has a roughly 79 px circular treatment; the font mark is about 48 x 39 px. These tools have no labels underneath. The top bar ends at y=178.

The current page stays at its existing coordinates. Its final line is hidden by a bottom panel that starts with a black 1 px rule at y=1344. The centered page-position label has 21 px caps around y=1410 to 1432. The scrubber track spans x=130 to 1133 at y=1512, with a 5 px filled portion and an outlined circular knob about 48 px in diameter, centered near x=172. End-navigation icons sit near x=84 and 1180 at the same y. The bottom action row has a contents-list icon near `73, 1590`, a bold current-chapter label beginning near `141, 1597`, an annotations icon at `1072, 1580, 44, 49`, and search at `1149, 1580, 49, 49`. Chapter caps are about 25 px. The panel ends at y=1680. White panels have no rounded outside corners or gray frame.

Controls and flow. Center tap opens this menu. Home returns to the application home. Pen opens or changes writing mode; rotation opens orientation choices; light, font, settings and More open the captured destinations. The slider seeks through the book. The edge icons offer navigation toward the beginning/end; whether they jump by chapter or book boundary needs a device check. Contents/current chapter opens the contents dialog, the paper icon opens annotations and search opens search. Expected dismissal is a tap on exposed page or the menu zone; verify the exact route and whether seeking preserves an undo position. A screenshot does not establish page-key handling while this menu is open.

## Font panel, `reader-font-settings.png`

Purpose. Change typography while retaining a large live page preview.

Layout. The reader toolbar stays at y=0 to 178 and the font icon receives a black 70 px disk with white lettering. The panel spans full width, y=179 to 740, with a black 1 px bottom rule. Labels begin at x=85 and use sans-serif caps about 24 to 25 px. The font dropdown is `338, 239, 573, 80`, a square 2 px black outline, with text at x=366 and a down chevron near x=871. The advanced action starts near `962, 268`, bold caps about 25 px.

Size, line spacing and margin rows have centers at y=372, 465 and 557, approximately 92 px apart. Their minus marks center on x=456 and plus marks on x=1175. Slider tracks run x=519 to 1112. Unfilled tracks are 1 px and filled tracks are 5 px. The approximately 48 px outlined knobs have centers x=761, 737 and 747 respectively. No numeric size or spacing value is shown. Alignment occupies `428, 609, 776, 83`, divided into three equal segments. The first is selected, dark with white bold text; the other two show left-align and justify icons on light gray. The page remains visible below y=741, and its ordinary footer returns at y=1612.

Controls and flow. The toolbar's font control opens this panel. The dropdown opens the font list. Advanced opens the font preview dialog. Tap minus/plus or drag a knob to change each value. The segments select book alignment, left alignment or justification. The first segment means no reader alignment override, not necessarily ragged-right prose. Tap the font control again or the page to dismiss, subject to device confirmation. Opening Light can stack its popover over this panel, as captured. Slider step sizes, endpoints, live-reflow timing and whether layout changes persist per book remain unmeasured.

## Font list, `reader-font-list.png`

Purpose. Pick the document's font or a reader font, with each choice previewed in its own face.

Layout. The font panel remains behind a square white dropdown at `347, 248, 628, 1202`, with a 3 px black border. Rows have about 91 px pitch, with thin gray rules from x=362 to 959 at y=348, 439, 530, 621, 712, 803, 894, 985, 1076, 1167, 1258 and 1349. Text begins at x=448. The first row has a check near `393, 295`, about 23 x 18 px, marking the document/default choice. Twelve entries are visible, including the default row. Their caps vary around 24 to 29 px because each face previews itself. A down chevron centers near `661, 1396` in the bottom 98 px area.

Controls and flow. Tap the font dropdown to open it, then a row to select its face and return to the font panel. The lower arrow exposes more choices; drag/scroll support and outside-tap dismissal need confirmation. Kaia should list its own installed and licensed fonts in this geometry. Proprietary Kobo and Rakuten font names in the capture identify evidence only and are not fonts to bundle. This capture does not expose the complete list or selection after a change.

## Advanced font dialog, `reader-font-advanced.png`

Purpose. Compare current and proposed font appearance before applying a change.

Layout. The shared 25 px inset dialog has a centered font-settings caption and close cross. The main heading begins near `124, 204`, 39 px caps. A gray strip occupies `100, 280, 1064, 49`. One line of explanatory text sits near y=362, with about 25 px caps. The selected-font dropdown is `455, 427, 709, 80`; its text is sans serif, about 27 px caps. Size and weight sliders run x=554 to 1064 at y=558 and 649. Minus and plus center near x=484 and 1127; knobs are about 48 px wide at x=763 and 771. Tracks have the same 1 px/5 px treatment as the font panel.

The comparison box is `100, 710, 1064, 744`, with a 2 px black border and a center division at x=632. Gray column headers are 49 px tall. Each half shows a clipped sample in the selected font, about 38 px cap height, with roughly 52 px line pitch on the left and 85 px on the right in this particular capture. Do not copy its sample text. Gray rectangular Restore and Apply buttons sit at `789, 1524, 214, 80` and `1013, 1524, 175, 80`. Their italic caps are about 24 px. The preview currently names a different font from the earlier default-font capture, so these images are not evidence of one unchanged font session.

Controls and flow. Open through Advanced in the font panel. The dropdown selects a face; the sliders change size and weight. Restore resets the proposed adjustment; Apply commits it. Close leaves the dialog, but cancel/apply persistence and Restore's exact baseline need a device check. Preview scrolling is not established. Recreate the comparison with Kaia's own sample and fonts, not the proprietary branding on the strip.

## Reader light popover, `reader-light.png`

Purpose. Adjust brightness and natural-light scheduling without leaving the book.

Layout. The active light icon is a black disk centered near `932, 128`. Its callout tip rises near y=174. The square popover is `451, 182, 803, 869`, with a 3 px black border, over the still-open font panel. The first heading begins at `503, 247`, serif caps about 30 px. The brightness value is centered near x=731, y=350, 32 px high. The brightness track spans x=578 to 1126 at y=423, with sun endpoints centered near x=532 and 1165. Its 48 px knob centers at x=731. A 1 px gray rule spans the panel at y=524.

The natural-light heading begins near `503, 590`, with about 31 px caps. An Auto label sits near `1007, 594`, 20 px caps, beside an on switch `1110, 579, 88, 49`. A second track runs x=578 to 1126 at y=765, with day/night symbols near x=532 and 1165 and knob centered near x=1064. The night-time row begins near y=865, with the value at x=748 and a dropdown chevron at x=874. A rule at y=949 separates an informational action at x=488, y=988, from the controls above. Ink is black on white; no warmth-colored panel appears.

Controls and flow. Open from the toolbar light icon. Brightness is 26 percent. Auto natural light is on, night time is 23:00, and the warmth knob is near the warm end. Drag either slider, change Auto or open the night-time selector. The bottom action opens an explanation. Expected dismissal is the light icon or a tap outside. Verify whether moving warmth disables Auto or merely overrides it temporarily. The settings also enable left-edge brightness swiping from the reading page. No numeric warmth value is visible.

## Reader settings page one, `reader-settings.png`

Purpose. Configure reading progress and page-button direction.

Layout. Use the shared large settings dialog, with no back arrow in this reader entry route. The heading is near `124, 204`, and an italic page counter sits near `960, 222`, with 23 px caps. The first gray strip is `100, 280, 1056, 49`. Header and footer rows occupy y=329 to 434 and 434 to 540. Labels begin at x=125; values align toward x=1080 and chevrons center on x=1118. Rules are at y=433 and 539. The progress checkbox is `928, 573, 38, 38`; its label is on the left and state text near x=991. A two-line explanation begins near y=681 with about 43 px pitch.

The button-control strip is y=783 to 832. A portrait-device diagram is roughly `346, 911, 345, 388`, with 4 px outlines. Two action labels lie to its right near x=765, y=1039 and 1152. The two-way direction selector is `136, 1368, 984, 90`, with a 5 px outline and modest rounded corners. The left half is black with white text and selected; the right is white with black text. The scroll thumb spans about y=298 to 733.

Controls and flow. The reader gear opens this dialog. Header and footer dropdowns choose progress types; the checked box enables the book progress strip. The direction segments choose default or reversed hardware buttons. This capture shows chapter pages left, book pages left, progress on and default buttons. Scroll or page to the next two settings states; the exact gesture and key mapping need verification. Close returns to reading. The device illustration is explanatory, not a separate control.

## Reader settings page two, `reader-settings-2.png`

Purpose. Configure page refresh, orientation, numbering and color display.

Layout. The outer dialog, caption, main heading and page-counter positions match page one. The gray page-appearance strip occupies y=280 to 329. Five rows follow at approximately 106 px pitch, with labels at x=125 and values/checks at the right. Row rules lie at y=433, 539, 645 and 751. Refresh and orientation values have dropdown chevrons near x=1118 at y=382 and 488. Checkboxes for Adobe numbering, dark mode and rainbow reduction occupy `928, 573, 38, 38`, `928, 679, 38, 38` and `928, 785, 38, 38`. A one-line caution about the last option sits near y=898. Text sizes follow the shared settings metrics. The thumb lies roughly y=734 to 1169.

Controls and flow. Reach this page by paging/scrolling reader settings. Dropdowns choose refresh interval and orientation. Checkboxes toggle marginal Adobe numbering, dark mode and rainbow reduction. The captured values are every chapter, automatic rotation, numbering on, dark mode off and rainbow reduction off. The repeated enable label beside an empty checkbox does not mean the setting is on. Close returns to reading; the same scroll track reaches pages one and three. Dropdown contents are not captured.

## Reader settings page three, `reader-settings-3.png`

Purpose. Configure touch areas and reading gestures.

Layout. Use the same outer dialog and title positions. A gray strip is y=280 to 329. Two explanation lines occupy roughly y=364 to 434, at 43 px pitch. Four tap-zone diagrams sit at x=157, 421, 685 and 949, y=482, each 150 x 200 px. The first has a roughly 10 px black selected border. Others have a gray outer surround and thin black inner outlines. The first diagram has three vertical zones of roughly equal width: back, menu, forward. The second favors forward taps with a small lower menu region; the third reverses the side preference; the fourth uses upper, middle and lower horizontal zones. Diagram fractions are not measured hitbox contracts.

A rule lies at y=697. The pinch row has its box at `928, 729, 38, 38`, with a rule at y=801. The left-edge brightness row wraps to two lines around y=841 and 884, with its box near `928, 855` and a rule at y=948. The final page-turn dropdown is around y=989 to 1015 with its chevron near x=1118. Standard setting text has 24 to 25 px caps. The thumb is near y=1169 to 1604.

Controls and flow. Select a diagram to choose a tap layout. Pinch sizing and left-edge brightness swipe are checked. The page-turn dropdown permits both taps and swipes in this state. Reach this page from the reader settings scroll sequence and close with the cross. Test actual zone boundaries, gesture direction, size increments and edge width on the device. No further gesture behavior is established by the pictures.

## Reader More popover, `reader-more.png`

Purpose. Expose secondary book actions.

Layout. The reader menu remains behind a top-right callout. More has a black 70 px active disk centered near `1199, 128`. The tip reaches y=174. The popover is `713, 182, 541, 383`, with square corners and a 3 px black outline. Four rows have about 91 px pitch, with rules from x=728 to 1238 at y=282, 373 and 464. Text begins at x=758 and y=230, 321, 412 and 503. Italic serif caps are about 25 px. There are no row icons, checkboxes or footer buttons.

Controls and flow. The four visible actions open book details, toggle the owner's screenshot facility, open the dictionary, and open reading help/tips. The screenshot entry is a local customization and its enabled state is not shown. Open via the toolbar ellipsis. Select a row to act; expected dismissal is the ellipsis or an outside tap. Nested destinations, long press and swipe behavior are not captured. Keep Kaia-only secondary tools accessible without treating this four-row custom menu as the complete product action list.

## Table of contents, `reader-toc.png`

Purpose. Jump to a section of the current book.

Layout. The shared inset dialog shows the book title centered near y=71 to 94. Tabs begin at x=83 and 226 near y=168, with about 24 px sans-serif caps. The selected contents tab is bold and red, with a 6 px underline from x=72 to 187 at y=213 to 218. A black rule spans x=72 to 1191 at y=219. The list has 16 visible rows at exactly 82 px pitch, from y=220 to 1532. The row rules run x=93 to 1117 and are 1 px `#aaaaaa`. Top-level text begins at x=199, children at x=270, a 71 px indent. Text has about 23 to 25 px caps, 17 to 18 px x-height, and up to 32 px total ink height. The current section is bold. No row page numbers appear. A scrollbar at x=1167 has arrows near y=270 and 1578 and an outlined thumb about y=331 to 726.

Controls and flow. Open through the menu's contents/current-chapter action. Tap a row to jump to that location. The other tab opens annotations. The close cross returns to reading without selecting a section. Scrollbar/arrows expose the rest of the hierarchy; swipe and page-key scrolling need confirmation. Preserve the hierarchy and current-section emphasis. The book's section names are content, not fixed UI labels.

## Annotations, `reader-annotations.png`

Purpose. Find this book's highlights, notes and bookmarks in one place.

Layout. The same dialog and title remain. The second tab is selected, with a red underline from x=209 to 421 at y=213 to 218 and bold red text. Beneath the y=219 rule, the filter begins at x=98, y=267, with its chevron near x=310. The sort control is a light-gray capsule `354, 246, 461, 64`, with text at x=379 and a chevron near x=779. The overflow dots center near `1152, 278`. Filter and sort caps are about 25 px. The empty state is a centered two-line serif message at y=963 to 1031, about 43 px line pitch and 24 px caps. It explains that future annotations will appear here, without a large illustration or action button.

Controls and flow. Open directly from the reader menu's annotations icon or switch tabs from contents. Filter is set to all annotation types; sort is by book position. Both dropdowns and the overflow expose choices not shown here. The contents tab and close cross provide exits. No list rows exist in this capture, so row layout, annotation editing, selection and scrolling remain unmeasured. Write Kaia's own empty-state sentence.

## Word selection and dictionary, `reader-hold-word.png`

Purpose. Select text, save an annotation or look up a word while retaining reading context.

Layout. The normal reading header and page remain. The selected word occupies roughly `659, 762, 462, 101`, white text on black. Thin stems lead to circular drag handles centered near `660, 718` and `1119, 906`, about 39 px in diameter. The selection callout is `719, 296, 342, 379`, with a 3 px black outline and a downward tip near x=889, y=683. Four roughly 55 px color disks center at x=769, 850, 930 and 1010, y=352. The first has an extra black selection ring about 67 px wide. Sampled fills are yellow `#e8da54`, pink `#e887b6`, blue `#76c9e8` and green `#7ae871`. State is also shown by the ring. Rules lie at y=401, 490 and 579. Three approximately 89 px rows offer highlighting, adding a note and search, with text at x=759 and about 25 px italic caps. Search has a right chevron.

The dictionary panel is `31, 1002, 1202, 647`, with a 1 px outline. Its language/dictionary selector starts at `82, 1047`, with a chevron near x=403. A rule runs x=57 to 1206 at y=1103. The result region starts near x=104, y=1145. Body caps are about 25 px with 45 to 48 px line pitch; the suggested headword is bold and larger, around 31 px caps. This result reports no exact match and offers a nearby entry. Do not reproduce its definition or publisher text. The bottom action strip begins at y=1558; encyclopedia and web icons sit near x=84 and 195, and expand near x=1177, y=1604. The original book progress track remains visible at y=1663.

Controls and flow. Holding a word produces the selection and dictionary state. Drag either handle to adjust the range. Choose a color, save a highlight, add a note, or open the search submenu. The dictionary selector changes the lookup source; the lower actions offer encyclopedia/web lookup and expanded results. Network actions in Kaia must retain B-92 opt-in behavior. Expected dismissal is a tap on unselected page; verify whether it closes both panels and whether changing the selection reruns lookup. The entry shows approximate-match handling, but no timing or lookup algorithm is proved here.

## Reader search and keyboard, `reader-search.png`

Purpose. Enter a query in the current book, with a visible search scope.

Layout. A gray surround frames a white search dialog from `25, 25` to y=1040. It has a back arrow near `62, 65` and a centered title at y=71, 23 px caps. There is no close cross in this capture. The query box is `61, 137, 1142, 86`, with a 3 px black outline. A magnifier is at `98, 162, 37, 37`, the caret near x=162, and gray italic placeholder ink begins at x=166, y=165, about 38 px high including descenders. A vertical divider at x=879 separates the current-reading scope dropdown, with a chevron near x=1146. The book selector begins near `85, 270` and its chevron is near x=393. Result space below is empty.

The keyboard spans the full screen width from y=1040 to 1680, separated by a 2 px black rule. There are five rows with centers near y=1134, 1250, 1371, 1488 and 1603. The top row contains ten digits; their centers run x=114 to 1148 at about 115 px pitch. Letter rows follow a QWERTY layout, with an at-sign on the second letter row and a period on the third. Letters have roughly 35 to 41 px ink height. Keys have no individual boxes. Shift and backspace occupy the outer ends of the third letter row. The final row contains symbols, accented-character mode, a space key shown by an underline from x=439 to 709, left/right caret arrows and a black Go key at `985, 1553, 214, 98`, with white text.

Controls and flow. Open from the reader menu search icon or a selection search route. Type into the query, change scope/book using the dropdowns, move the caret with the arrows and submit with Go. Shift, symbols, accents, backspace and space perform their standard text-entry actions. The back arrow returns to reading. The later global-search captures show scope choices and one library result. In-book results, history, keyboard dismissal and accent long press remain uncaptured. Spanish interface text alone does not prove a Spanish-specific alphabet layout; the visible key set is what is recorded here.

## Home, `home.png`

Purpose. Resume a book and reach library groups or other destinations.

Layout. Use the shared application status row and bottom navigation. No app-name heading appears below the clock. The current cover has a thin gray frame at `80, 237, 256, 339`. Its progress caption begins near `78, 611`, with about 30 px serif caps. There is no separate title, author, horizontal progress bar or time estimate beside this card in the captured one-current-book state. The upper-right area stays white.

Two lower cards start at x=79 and 644, y=852. Each cover stack occupies about 540 x 264 px. The leading cover box is about 264 x 264 px here because the imported junk entries are square; normal book aspect-ratio behavior needs valid-library captures. Text headings begin near y=1151 with about 30 px caps. Gray uppercase count/status text begins near y=1211 with about 20 px caps. Rules at y=1270 divide the cards from two text-only promotional blocks below. These begin at x=78 and 644, y=1310, with about 30 px caps and 54 px line pitch; small gray service labels sit below. The bottom navigation starts at y=1554 with Home selected. Cover colors are content; active chrome uses the shared red accent.

Controls and flow. The current cover resumes reading. The two lower cards open all books and an author group. Status icons open light, Wi-Fi, battery, sync and search. Bottom tabs navigate to their named destinations. Promotional cards open Kobo services and stay outside Kaia. Home is reached by its bottom tab or the reader's return action. There is no close button. Long-press card behavior and home scrolling are not shown. The junk import does not justify copying its images or library counts.

## Home alternate state, `home-after.png`

Purpose. Record another home state, including changing promotional content.

Layout. The cover, 3 percent caption, two lower card stacks, margins, font sizes, rules, status row and bottom navigation use the same coordinates as `home.png`. The clock differs. The lower-right text block now promotes saved web articles, with two text lines around y=1310 and 1364 and its service label around y=1424. This is content variability, not a second grid layout.

Controls and flow. All Home controls and routes apply. The changed promotional destination is outside Kaia's copied service scope. Do not infer that the filename proves a navigation sequence or that this card must always exist. No additional gestures are established.

## Home with two books, `home-two-books.png`

Purpose. Resume either of two recently opened books.

Layout. The normal status and bottom navigation remain in their shared positions. The recent row has two independent covers: a white PDF tile at `80, 202, 256, 374` and an EPUB cover at `362, 237, 256, 339`, aligned at y=576. The column advance is 282 px, leaving a 26 px gap between 256 px covers. Progress captions begin near `78, 611` and `360, 611`, with about 30 px caps. They show 6 and 3 percent respectively. These are reading states, not defaults. There are no adjacent titles, author lines, horizontal progress bars or time estimates. The lower library/group cards retain their x=79 and 644 columns, y=852 image row, y=1151 headings and y=1211 counts. Shared rules, colors and typography apply; white is also the PDF cover content and is not evidence of a disabled tile.

Controls and flow. Tap either cover to resume that document. This state follows opening an additional book and returning Home; the filename alone does not establish the exact ordering rule. The lower cards, status popovers and bottom destinations retain the Home actions. No recent-row paging or swipe affordance is visible with two books. Verify ordering and overflow with more books; no special close action applies to Home.

## My books, `mybooks.png`

Purpose. Browse the library and switch between books, authors, series and collections.

Layout. The shared status row sits above four text tabs at x=79, 220, 385 and 525, y=159. Caps are about 25 px; the selected first tab is bold red with a 6 px underline at `67, 204, 117, 6`. A thin rule reaches x=1197 at y=210. Filter and sort dropdowns begin near `92, 257` and `374, 257`, with about 25 px italic serif caps and chevrons near x=305 and 689. Overflow dots center near `1157, 269`.

The grid has four columns starting at x=75, 344, 613 and 882, with boxes about 254 px wide and 15 px horizontal gaps. The first real book is `75, 441, 254, 336`; the other first-row images are square and bottom-align at y=777. Their common top is y=523. Status captions start around y=814 and per-book overflow dots around y=875. The second row of square images is y=1098 to 1352, with status at y=1390 and dots near y=1450. Status caps are about 25 px. A thin black scroll track sits at x=1194 from roughly y=440 to 1418, with up/down arrows near y=378 and 1479 and a thumb near y=439 to 496. The bottom navigation highlights books.

Controls and flow. Open with the books tab or Home's library card. The four upper tabs choose grouping. Filter is all books and sort is recent. Their dropdowns and the top ellipsis change the view or library options; the later filter, sort and view-options captures below show their entries. Tap a cover to open it and its ellipsis for book actions. Scroll/arrows expose more rows. Bottom navigation leaves the library. Long press and multi-select need a device pass. The later list/menu pair shows the grid/list alternatives. The odd square covers are imported files, not target book artwork.

## Author library, `library-author-unknown.png`

Purpose. Browse the books in one author group.

Layout. The normal status row remains. A back arrow near `77, 148` and a sans-serif author heading at `141, 152`, 25 px caps, replace the four category tabs. A right-side italic action begins near `845, 152`, about 25 px caps. The heading rule is at y=198. Filter/sort/overflow follow near y=245 to 270. The four grid columns match My books, but the square images begin at y=511 and 1086. Status captions sit near y=803 and 1378, and ellipses near y=863 and 1438. The scroll arrows center near y=366 and 1479; its thumb is near y=427 to 493. The books bottom tab remains selected.

Controls and flow. Reach this group from Home's author card or an author list. Back returns to the prior library/home context. Filter, sort, cover taps, per-book overflow and scrolling work as in My books. The right-side action offers more books by that author and may lead to a store-backed destination; its actual route is not captured and Kaia should not invent a server dependency. The unknown author is junk-import metadata, not a special product group to hardcode.

## More tab, `more-tab.png`

Purpose. Reach secondary application destinations.

Layout. Use the shared application status row. The main serif heading starts near `89, 145`, with about 39 px caps. A black 1 px rule spans x=67 to 1196 at y=221. Nine rows each have 134 px pitch, with rules at y=355, 489, 623, 757, 891, 1025, 1159, 1293 and 1427. Icons begin near x=85 and occupy about 43 to 48 px. Text starts at x=172 and y=277, 411, 545, 679, 813, 947, 1081, 1215 and 1349. Italic serif caps are about 25 px, with total ink height around 33 px. The bottom navigation highlights More. Most icons are black outlines; external service logos introduce blue, green/yellow and pink. Those logos are not Kaia assets.

Controls and flow. Rows open favorites, view-only notebooks, Dropbox, Google Drive, saved web articles, activity/statistics, beta features, settings and help. Cloud/account-linked destinations remain Nickel-only under B-103. Open More from bottom navigation; leave through another bottom tab or open a row. No paging, close button or gesture beyond tapping is visible. Kaia can route its existing local favorites, notes, statistics, experiments, settings and help through equivalent rows with open icons.

## NickelMenu, `nm-menu.png`

Purpose. Record the owner's external launcher and test controls.

Layout. The More page stays behind a lower-right white popover at `810, 1071, 445, 474`, with a 3 px black outline. Five rows have about 91 px pitch. Gray rules run from x=825 to 1239 at y=1171, 1262, 1353 and 1444. Italic serif labels start at x=852 and y=1118, 1209, 1300, 1391 and 1482, about 25 px caps. The menu ends just above the application navigation. No pointer tip or close button is visible.

Controls and flow. The installed NickelMenu bottom item opens this launcher. Its rows launch the Lua app, launch the Rust app, enable remote access, disable remote access and toggle screenshots. Action completion and outside-tap dismissal are not shown. These controls belong to the owner's external launcher; matching Nickel's design does not require embedding remote administration or NickelMenu in Kaia.

## Additional More capture, `x.png`

Purpose. Account for the otherwise unnamed PNG inspected before it was removed from the input directory during the review. This is historical evidence, not a current inventory file.

Layout. It shows the More page without the NickelMenu popover, using the same status row, 134 px list pitch, nine destinations, typography, rules, icons and bottom bar described for `more-tab.png`. Its clock reads a later time. There is no extra dialog, error state or new control to reproduce.

Controls and flow. The More page controls apply. The filename does not establish what closed the previous menu. Dismissal and gesture behavior therefore remain unverified.

## Settings index, `settings.png`

Purpose. Choose a device/application settings category.

Layout. Use the shared inset dialog without a back arrow. The centered caption and main heading both identify settings. The heading has a measured 39 px capital height; some full glyph bands reach 53 px due to ascenders/descenders. Eleven rows extend from y=280 to 1458 at exactly 107 px pitch. Rules lie at y=280, 387, 494, 601, 708, 815, 922, 1029, 1136, 1243, 1350 and 1457, spanning x=100 to 1156. Labels begin at x=125, with about 30 px italic serif caps and up to 42 px total ink height. The page has no row icons, boxed buttons, application status row, bottom navigation or visible scrollbar.

Controls and flow. Rows open accounts, date/time, language/dictionaries, Wi-Fi, Bluetooth, sync/updates, power/privacy, reading settings, download management, device information and device/about help. The More page settings row opens this index. Close returns to its caller. Accounts, Kobo sync and service downloads are evidence of organization, not required Kaia services. Only the supplied subpage captures below establish their visual details. No swipe behavior is visible on this index.

## Power settings page one, `settings-power.png`

Purpose. Set sleep, shutdown, wake and sleep-screen behavior.

Layout. Use the shared settings subpage shell, back arrow and close cross. The long heading begins at x=124, y=205; an italic page counter begins near x=960, y=222. Gray strips occupy y=280 to 329, 515 to 564 and 1030 to 1079. Timeout rows are y=329 to 422 and 422 to 515, 93 px each; right dropdown chevrons center near x=1118. The wake-button row wraps at about y=579 and 622, ending at y=661. Subsequent rows end at y=754, 847, 940 and 1030. Their checkboxes begin at x=928 with centers near y=613, 707, 800, 893 and 984. The cover-sensor checkbox centers near y=1126. Three explanation lines follow at y=1188, 1231 and 1274. Text follows the common 24 to 25 px cap height and 43 px paragraph pitch. The scroll thumb extends roughly y=298 to 951.

Controls and flow. Open the power/privacy row in Settings. Sleep is 5 minutes and shutdown is never. Checkboxes enable page-button wake, showing the current book, full-screen covers, the cover's details panel, the charging indicator and automatic sleep/wake from the magnetic cover; all are checked. Dropdowns change timeouts. Back returns to the index and close leaves settings. Scroll to the second page. The sleep screen itself is not captured, so cover fitting and details-panel geometry are not established here. Hardware wake and cover gestures still need physical verification.

## Power settings page two, `settings-power-2.png`

Purpose. Configure lock and privacy options.

Layout. The same heading and shell show page two. Section strips lie at y=280 to 329, 655 to 704 and 794 to 843. The PIN enable row ends at y=421, with an empty `928, 356, 38, 38` box. The change-PIN row ends at y=515 and uses pale gray text, about `#bfbfbf`. Three explanatory lines occupy about y=531 to 641, with 43 px pitch. A checked discovery-history box sits near `928, 729`, followed by a checked usage-sharing box near `928, 873`. Its label wraps to two lines. A rule lies at y=939. The scroll thumb spans roughly y=951 to 1604. Font metrics match page one.

Controls and flow. Page here from power page one. PIN is off, so changing the four-digit PIN is disabled. The two other captured checkboxes enable store browsing history and automatic usage reporting. Those are Nickel states, not Kaia defaults or requirements. Kaia has no telemetry and should omit the reporting control. Back and close retain their shared meanings. Enabling PIN and entering/changing it are uncaptured flows; do not infer the keyboard or confirmation layout.

## Reading settings from Settings, `settings-reading.png`

Purpose. Reach the same reading preferences outside an open book.

Layout. Geometry, default values, progress rows, button diagram, two-way direction selector and scroll thumb match `reader-settings.png`. The extra visible control is a back arrow near `62, 65, 36, 32`. The common dialog caption, main heading, page count, 49 px section strips, 38 px checks and font metrics otherwise match reader settings page one. No application status row or bottom bar is visible.

Controls and flow. Open the reading category in the settings index. Header/footer selectors, progress checkbox and default/reversed button choice behave as described for the reader entry route. Back returns to the index; close returns to the prior application page. The reader entry route has no back arrow, so the two entry paths must not share a hardcoded return destination. Scroll reaches the other reading pages. Their outside-reader variants are not separately captured.

## Wi-Fi settings, `settings-wifi.png`

Purpose. Control the radio and manage saved network connections.

Layout. Use the shared settings subpage shell. The heading has 39 px caps near `124, 205`. Gray strips are `100, 280, 1064, 49` and `100, 579, 1064, 49`. The checked Wi-Fi box is `936, 365, 38, 38`; its on label begins near x=999. A rule lies at y=430. Two lines of explanatory text begin near y=467 with about 43 px pitch. Connect and remove-network rows extend from y=629 to 741 and 741 to 843. Labels begin at x=125 and right-aligned italic actions near x=989 and 998. Three more explanation lines occupy roughly y=879 to 989. Setting caps are 24 to 25 px. No scrollbar or network list is visible.

Controls and flow. Open from Settings' Wi-Fi category; the later Home Wi-Fi popover provides a settings action as another route. The checked control enables Wi-Fi. Connect opens network selection; Remove opens saved-network removal. Back returns to Settings and close leaves it. The later Home Wi-Fi popover shows network scan rows. Credential keyboard, failures and captive portals are uncaptured. No network side effect was tested during this research.

## Language and dictionaries, `settings-language.png`

Purpose. Choose the interface language and manage dictionaries.

Layout. Use the shared settings subpage shell. The main heading begins at x=124, y=205, with 39 px caps. Gray section strips occupy y=280 to 329 and 430 to 479. The language row shows its label at `125, 364`, value near `977, 364` and right chevron near x=1134, y=375. The dictionary-count row begins at y=512 and ends with a rule at y=570. Its italic Edit action is right-aligned near x=1034. A total-size row begins at y=613, with the value aligned toward x=1156. Caps are 24 to 25 px. The rest of the page is blank white; no scrollbar appears.

Controls and flow. Open the language/dictionaries category. Language is Spanish; two dictionaries occupy 13.3 MB in this device state. Tap the language row for selection or Edit to manage dictionaries. Count and total size are read-only. Back returns to the index and close exits settings. Available languages, download prompts, deletion and dictionary order are not captured. Kaia should preserve its own approved dictionary acquisition policy.

## Date and time, `settings-datetime.png`

Purpose. Inspect or change date, time, time zone and clock format.

Layout. Use the shared settings subpage shell. The heading is at x=124, y=205 with 39 px caps. There are four rows between y=280 and 733 at 113 px pitch, with black rules at y=280, 393, 506, 619 and 732. Labels start at x=125 around y=326, 439, 552 and 665. Date/time/zone values align at the right edge around x=1134, with 24 to 25 px italic caps. The 24-hour control is an 88 x 49 px on switch near `1073, 652`, with its state label to the left in 20 px sans-serif caps. Three explanatory lines start near y=794 at 43 px pitch. No scrollbar appears.

Controls and flow. Open the date/time category. The capture shows October 15, 2026, 02:51, UTC zero and 24-hour time on. Date, time and zone rows expose adjustment routes by their action styling; their editors are not shown. The switch changes clock format. Back returns to the index and close exits. Nickel's text describes automatic network clock adjustment; Kaia retains B-90's opt-in network time. No automatic synchronization was observed.

## Device information, `settings-device-info.png`

Purpose. Inspect device/software facts and reach maintenance or legal information.

Layout. Use the shared settings subpage shell. The main heading starts at x=124, y=205, 39 px caps. Section strips are y=280 to 329, 748 to 797 and 990 to 1039. Hardware rows show serial number, MAC address, IP address and available/total storage, with rules at y=431, 534 and 637 and about 103 px pitch. Values right-align toward x=1156; identifier values are intentionally not transcribed here. The software version row occupies y=797 to 900, and update action y=900 to 990. Five advanced rows follow, ending at y=1130, 1221, 1312, 1403 and about 1494. Labels start at x=125 and italic actions align at the right. Caps are 24 to 25 px. There is no visible scrollbar.

Controls and flow. Open the device-information category. Hardware facts and version are read-only. Captured storage is 26.8 GB available out of 32 GB; firmware is 4.42.23303. The actions check for updates, repair a Kobo account, factory-reset the device, open legal text, open warranty text and open firmware information. Back returns to Settings; close exits. Kaia should display its own build and device facts, retain its signed-update policy, and use the two scoped reset actions required by B-84. The Kobo account repair and whole-device reset are not Kaia actions. Confirmation screens and external destinations were not captured.

## Home light popover, `home-light.png`

Purpose. Adjust the same lighting settings from Home.

Layout. The application status row and Home remain behind a white callout at `363, 139, 803, 869`, with a 3 px outline and a tip under the active light icon at x=765. This is the reader-light panel shifted 88 px left and 43 px up. The brightness heading begins near `415, 205`, about 30 px caps. The 26 percent label is near `610, 307`. Tracks run x=490 to 1039 at y=380 and 722, with 48 px ring knobs centered at x=643 and 976. The Auto switch is `1022, 536, 88, 49`; its label has about 20 px caps. The night-time row shows 23:00 near y=823. Rules at y=481 and 906 separate brightness, natural light and the bottom information action. The page and bottom navigation remain visible around it.

Controls and flow. Tap Home's light icon to open. Brightness, warmth, Auto, night time and the information action have the same purposes as in the reader panel. Expected dismissal is the icon or an outside tap. This pair establishes a reusable panel with different anchors, not a second lighting configuration. Slider gesture timing and manual/Auto interaction remain untested.

## Home Wi-Fi popover, `home-wifi.png`

Purpose. Toggle Wi-Fi, choose a nearby network or reach full Wi-Fi settings.

Layout. A 3 px square callout occupies `324, 139, 930, 939`, with a tip at x=863 under the active 80 px status-icon disk. The top row ends at y=255. Its on switch is near `380, 174, 88, 49`, followed by an uppercase status label with about 20 px caps. Five networks appear below. Signal icons center near x=395, lock icons near x=477, and network-name text begins at x=545 with about 25 px serif caps. The first row has a connected-state line below in 20 px sans-serif caps and a check near x=1157. Rules lie at y=401, 538, 675 and 812; ordinary rows have 137 px pitch. The right track is at x=1219, with a thumb from about y=283 to 445. A rule at y=960 separates the full-settings action, beginning near `352, 1008`, with 25 px italic caps. Names of private networks are not transcribed.

Controls and flow. Tap the status Wi-Fi icon to open. Toggle the radio, tap a network to connect, scroll to other networks or open Wi-Fi settings from the bottom row. Locks distinguish secured entries and the check identifies the active one. Close through the icon or outside tap, subject to device confirmation. Password entry and connection failures are not shown.

## Home battery popover, `home-battery.png`

Purpose. Show battery charge and a shortcut to power settings.

Layout. A square white callout is `585, 139, 634, 239`, outlined in black at 3 px with its tip under the active battery disk at x=960. The upper row reports 48 percent near `639, 189` with about 27 px sans-serif caps. A 1 px gray rule lies at y=258. The lower power-settings action starts at `639, 306`, with 25 px italic serif caps. No slider or editable number appears. Home, the status row and the bottom navigation remain visible.

Controls and flow. Tap the battery icon to open. The percentage is read-only; the lower row opens power settings. Expected dismissal is tapping the battery icon again or outside. No charging-specific state or gesture is captured. The 48 percent value is a momentary reading, not a default.

## Home search, `home-search.png`

Purpose. Start a search outside a book.

Layout. The shell and keyboard match `reader-search.png`: 25 px inset upper dialog, back arrow, centered title, field near `61, 137, 1142, 86` and full-width five-row keyboard from y=1040. The query placeholder describes book metadata rather than a word in a book. The field's scope divider is at x=906 and the selected scope is the Kobo store. The placeholder is gray italic serif; its full ink height is about 38 px. There is no current-book selector below the field in this state, so the rest of the upper area is empty white. The Go key remains `985, 1553, 214, 98`.

Controls and flow. Open via Home's search icon. Enter a query, choose a scope and submit with Go. Back returns to Home. All keyboard controls have the same meaning as reader search. The reference store default is outside Kaia's scope; Kaia should start with its local library. No request was sent during this research.

## Search scope dropdown, `search-scope.png`

Purpose. Choose which data the query searches.

Layout. Over Home search, a square white menu occupies `823, 222, 380, 474`, with a 3 px black outline. Five rows have about 91 px pitch, with rules from x=838 to 1187 at y=322, 413, 504 and 595. Labels begin at x=923 with 25 px italic caps. A check near `869, 270` marks the store row. The remaining rows name the library, current reading, annotations and dictionary. The query field and keyboard stay visible, and the field border is thinner while the dropdown holds focus.

Controls and flow. Tap the search scope chevron to open and a row to select its search domain. Verify whether unavailable domains are disabled without an open book. Expected dismissal is an outside tap or selecting a scope. Kaia should preserve the local domains it supports and omit Kobo store access. This capture establishes scope names, not the result layout for every domain.

## Library search result, `search-results.png`

Purpose. Select a matching local book after searching.

Layout. The inset search dialog now fills `25, 25, 1214, 1630`; the keyboard is hidden. The query field remains near `62, 139, 1139, 82`, with local books selected to the right of x=952. Query text has about 30 px caps. A relevance-sort dropdown begins at `85, 270`, with a chevron near x=292 and 25 px italic caps. One result occupies about y=366 to 574. Its empty cover frame is `84, 366, 124, 182`. The title starts near `230, 422`, about 30 px caps; author text near `230, 478` has about 24 px caps. Status and file-type/size align right near x=1182 at y=381 and 430, and overflow centers near `1161, 490`. A 1 px rule lies at y=574. The centered result-page indicator sits near y=1577 with about 23 px caps. The item is a PDF, not an in-book text hit.

Controls and flow. Submit a library-scoped query to reach this view. Tap the row to open the document, its ellipsis for book actions, sort to change ordering, or the field to edit. Back returns to the previous search context. There is only one result page, so pagination controls beyond the counter are not visible. In-book hit snippets and search history remain uncaptured.

## Later books grid, `library-books.png`

Purpose. Record the grid state used by the later library-menu captures.

Layout. It matches `mybooks.png`: shared status/nav rows, four category tabs at y=159, filter/sort at y=257, four image columns at x=75, 344, 613 and 882, first-row common bottom y=777, second row y=1098 to 1352, and right scrollbar. Cover widths are 254 px. Category caps are about 24 px; status captions about 25 px. The clock has advanced, but no new layout is visible. Junk cover contents remain excluded from the reference.

Controls and flow. The books tab opens this view. Category tabs, filter, sort, library overflow, cover taps, book ellipses, scrollbar and bottom navigation retain their meanings from My books. The next three captures show the previously unmeasured dropdowns. No new gesture is established by the duplicate grid.

## Library filter dropdown, `library-filter.png`

Purpose. Restrict the library by reading state or source/download state.

Layout. The filter trigger becomes black from x=67 to 339, y=211 to 329, with white italic text and an up chevron. The callout is `67, 327, 429, 565`, 3 px black outline, with a top tip centered near x=203. Six rows use about 91 px pitch; rules are y=427, 518, 609, 700 and 791, inset 14 px. Label text begins at x=168 and gray counts right-align around x=459. Caps are about 25 px. A check near x=124 marks the all-books row. The rows are all, unread, in progress, finished, imported and downloaded. The grid remains behind the menu.

Controls and flow. Tap Filter to open, then a row to apply that filter. Counts reflect the current library, including accidental imports here. They are not fixtures. Tapping the trigger or outside is the expected close route. No filter combinations or hidden extra entries are shown; the displayed groups should not be treated as independent checkboxes.

## Library sort dropdown, `library-sort.png`

Purpose. Choose the library ordering.

Layout. The sort trigger is black at `349, 211, 376, 119`, with white text and an up chevron. A callout at `349, 327, 445, 656` has a 3 px outline and a tip near x=536. Seven rows have about 91 px pitch, separated at y=427, 518, 609, 700, 791 and 882. Labels begin at x=450, with about 25 px italic caps. The selected recent-order row has a check near x=406. Choices after recent are added date, title, author, series, file size and file type. No ascending/descending button appears in this capture.

Controls and flow. Open from Sort, select a row to reorder the current view, or dismiss via the trigger/outside. The menu overlays the existing grid without a separate screen. Persistence, tie-breaking and direction behavior need a device check. The visible ordering options are now established; their algorithms are not.

## Grid view options, `library-view-options.png`

Purpose. Switch display mode or reach library management.

Layout. The library ellipsis becomes a 56 px black disk centered at `1157, 269`. The callout tip begins near y=300, above `746, 308, 451, 474`, a 3 px-outlined white box. Five rows have 91 px pitch and rules at y=408, 499, 590 and 681. Text begins at x=791 with 25 px italic caps. The first action switches to list view, followed by book management, download all, cancel downloads and download management. The middle two download actions are gray and disabled in this state.

Controls and flow. Open with the library ellipsis. The first action produces the list capture below; management opens selection/management tools not captured here. The enabled download-manager row reaches a separate page. Close through a selection or expected outside tap. Kobo-only download functionality is out of scope for Kaia; keep local library actions in equivalent positions without dead service buttons.

## Library list, `library-list.png`

Purpose. Browse books with title, author, progress and file metadata visible.

Layout. Status, category tabs, filter/sort and bottom navigation match the grid. Five rows fit below the controls, with 243 px pitch and rules from x=67 to 1143 at y=562, 805, 1047 and 1290. The first cover is `88, 363, 124, 164`; square junk thumbnails in later rows occupy 124 x 124 px. Main text begins at x=232. First-row title ink is near y=409 with about 30 px caps; author ink is near y=466 with about 23 px caps. Read state is right-aligned toward x=1124 near y=371 and file-type/size near y=417, about 25 px serif and 20 px sans-serif caps respectively. The first row's ellipsis centers near `1104, 478`; subsequent rows repeat this arrangement. The scrollbar remains near x=1194 with arrows near y=378 and 1479.

Controls and flow. Choose list view in library options. Tap a row or cover to open the book, its ellipsis for actions, or the shared filter/sort controls. Scroll to more rows. Bottom tabs leave the library. Only the first real book's proportions are useful for a valid-cover comparison; square junk thumbnails must not set the product's default cover ratio.

## List view options, `library-list-menu.png`

Purpose. Return from the metadata list to cover view or manage the library.

Layout. The menu is the same `746, 308, 451, 474` callout as grid view options, with the same 3 px outline, 91 px row pitch, thin rules, 25 px italic caps and disabled download rows. Its first action now offers covers. The list stays visible behind it, so this is a mode-dependent label and action rather than two separate menus with different geometry.

Controls and flow. Open from the list's top ellipsis. Choose covers to return to the grid; other actions match grid options. Selection or an expected outside tap closes the menu. No additional gesture or view preference persistence is proved by the pair.

## Book action menu, `book-menu.png`

Purpose. Act on one library book without opening its details page.

Layout. A square white callout is `144, 592, 460, 565`, with a 3 px border and a left-pointing tip near `136, 875`, beside the selected book's ellipsis. Six rows have 91 px pitch, with rules from x=159 to 588 at y=692, 783, 874, 965 and 1056. Labels begin at x=189, around y=640, with about 25 px italic caps. The rows open reading, show details, add to a collection, mark unread, mark finished and remove the book. There are no row icons, toggles or per-row secondary labels. The cover grid remains behind it.

Controls and flow. The visible anchor is the book ellipsis; long-press equivalence is not established. Select a row to perform its action or open the next step. Removal confirmation and collection selection are not captured. Expected dismissal is tapping outside. Kaia must keep its own book-data retention and deletion rules, not infer them from the word Remove.

## Book details summary, `book-details.png`

Purpose. Inspect book metadata and summary before reading.

Layout. The shared 25 px inset dialog has a library caption at y=71 and close cross. A cover frame is `81, 140, 285, 377`. Book title begins at `392, 151` with about 34 px serif caps; the author begins near `393, 220`, about 27 px sans-serif caps. A flat gray Read action is `393, 277, 249, 80`, with a gray 56 px circular ellipsis centered near `685, 317`. The lower section starts with two text tabs at x=85 and 259, y=811, about 24 px sans-serif caps. The summary tab is bold red with a 6 px underline at y=855; a rule spans x=72 to 1191 at y=861. Summary prose begins at x=72, y=898, with about 25 px caps and 45 px pitch, wrapping across the page. Its wording is book data and is not reproduced here.

Controls and flow. Open Details from a book menu. Read opens/resumes the book, the ellipsis opens status/collection/removal actions, tabs switch summary and metadata, and close returns to the library. Scrolling for a longer description is unmeasured. Preserve the cover and summary as user book data, not bundled screenshot assets.

## Book details metadata, `book-details-tab.png`

Purpose. Inspect identifiers, language, file location, size and format.

Layout. The cover, title, author, Read button and ellipsis match the summary view. The second tab is active, with its red underline x=243 to 389 at y=855 to 860. Six metadata lines begin at x=72 around y=898, 943, 988, 1033, 1078 and 1123. They use about 25 px serif caps and 45 px pitch. Fields are a book identifier under an ISBN label, language, file name, directory, file size and format. The identifier happens to be a UUID-style value. It and the owner's path are not transcribed as product fixtures. There are no row borders or edit controls.

Controls and flow. Select the metadata tab from Details. These fields are read-only. Summary, Read, overflow and Close retain their purposes. The screenshot does not show copying values, editing metadata or horizontal scrolling. Long identifiers and paths need wrapping or truncation checks in Kaia.

## Book details overflow, `book-details-more.png`

Purpose. Change a book's read state, collection membership or presence from Details.

Layout. The ellipsis at `685, 317` becomes black with white dots. A downward callout begins at `455, 371, 460, 383`, with a tip near y=363 and a 3 px black outline. Four rows at 91 px pitch contain unread, finished, collection and remove actions. Rules run x=470 to 899 at y=471, 562 and 653. Text begins at x=500 with about 25 px italic caps. Details remains visible behind the menu, including the metadata tab lower down.

Controls and flow. Open from the Details ellipsis. Choosing an action updates status or opens its next step. Close through selection or an expected outside tap. The shorter list omits Read and Details because they are already present in the parent dialog. Confirmation and collection-picker screens remain uncaptured.

## Authors tab, `library-authors.png`

Purpose. Browse grouped authors rather than individual covers.

Layout. The shared library tabs remain, with the second selected and its 6 px red underline from x=205 to 348 at y=204. A surname-sort dropdown begins at `92, 258`, with its chevron near x=464. Three author rows have 120 px pitch. Name ink begins near x=99 at y=373, 493 and 613, with about 30 px italic caps. Counts right-align toward x=1174 with about 25 px caps. Thin rules span x=67 to 1196 at y=447, 567 and 687. There are no covers or row ellipses. The bottom books destination stays selected.

Controls and flow. Tap Authors to open, select an author row to reach the author-filtered book view, or change sort. The unknown-author count reflects accidental imports. Other category tabs and bottom navigation leave the view. More authors may require scrolling, but this short list shows no scrollbar. A separate author-edit action is not visible.

## Series empty state, `library-series.png`

Purpose. Explain the absence of grouped series.

Layout. The third library tab is active with a red underline at x=370 to 486, y=204. No filter or sort row appears. An approximately 504 px cyan circle is centered at `632, 666`, spanning y=414 to 918. Its sampled fill is `#0ed0db`. A white outlined device illustration inside is about `498, 515, 268, 303`. It is proprietary artwork and must be replaced with an original/open drawing. A centered empty heading starts near y=987 with about 39 px caps. Two supporting lines around y=1076 and 1119 use about 25 px caps and 43 px pitch. A gray store action is `509, 1202, 246, 80`. Normal status and bottom navigation remain.

Controls and flow. Select Series to reach this empty library state. The visible action leads to Kobo's store, which Kaia excludes. Use an appropriate local-library action or omit it. Category/bottom tabs leave the page. The illustration and explanation are not controls. A populated-series layout is not captured.

## Collections empty state, `library-collections.png`

Purpose. Invite the reader to create the first collection.

Layout. Collections is the active fourth tab, with a red underline at x=508 to 713, y=204. The same 504 px cyan illustration circle is shifted upward, spanning x=380 to 884, y=348 to 852. Its device drawing sits around `498, 449, 268, 303`. The centered heading begins near y=921 with about 39 px caps. Five centered explanation lines occupy roughly y=1009 to 1208 at about 44 px pitch, with 25 px caps. A flat gray create action is `453, 1268, 359, 80`, with about 25 px italic caps. There is no filter/sort row or scrollbar. Status and bottom navigation remain.

Controls and flow. Select Collections. The create button opens collection naming, not shown here. Other tabs leave the view. Match the placement with Kaia's own concise explanation and original/open illustration. Do not copy the five-line wording to force identical line breaks.

## Favorites empty state, `more-favorites.png`

Purpose. Show an empty favorites list. The captured explanation ties this Nickel list to store titles.

Layout. A back arrow near `77, 154` precedes the heading at `148, 159`, with about 24 px sans-serif caps. A rule lies at y=210. Filter and sort begin near `92, 258` and `375, 258`, with 25 px italic caps. The 504 px cyan circle is centered at `632, 691`, spanning y=439 to 943, with the same original-to-be-replaced device artwork. The empty heading wraps to two lines near y=1012 and 1080, about 39 px caps and 68 px pitch. Supporting text occupies y=1169 and 1212, about 25 px caps. The gray store button is `509, 1295, 246, 80`. More stays selected in the bottom navigation.

Controls and flow. Open Favorites from More. Filter and sort choose list state/order. Back returns to More; bottom tabs leave the view. The store button is outside Kaia's scope. Kaia's existing local favorites can use the layout, but must not claim to synchronize this store-backed list. Populated favorite rows and add/remove behavior remain uncaptured.

## Notebooks introduction, `more-notebooks.png`

Purpose. Show the notebook destination with a stylus activation introduction.

Layout. Behind the popup is a notebook page with a back arrow near `101, 165`, a large heading at `165, 163`, about 39 px caps, a rule at y=233, recent-sort text near y=282 and overflow near `1157, 292`. The popup is `232, 290, 800, 1100`, white with a 3 px black outline and close cross near `973, 318, 30, 30`. A proprietary pen/device illustration occupies roughly x=442 to 833, y=400 to 657, with black linework and colored annotation marks. Three large heading lines begin near x=308, y=750, at about 68 px pitch and 39 px caps. Three visible sans-serif body lines begin around y=981 at 44 px pitch and about 25 px caps. The activation action begins near y=1139 and the stylus-information action near y=1240, with thin rules at y=1199 and 1301. The underlying More navigation remains visible.

Controls and flow. Open Notebooks from More. The dialog offers stylus activation, stylus information and Close. Closing should reveal the notebook list, as in the next capture. Do not recreate branded activation requirements or copy the illustration in Kaia. The actual device capability and Kaia's handwriting decisions govern available creation/editing functions. No activation sequence is captured.

## Notebook list under an activity filename, `more-activity.png`

Purpose. Record the actual image, which shows Notebooks after the introduction closes, not reading statistics.

Layout. The status row, notebook heading/back arrow, y=233 rule, recent-sort dropdown and right ellipsis match the background in `more-notebooks.png`. A new-notebook tile is `109, 373, 192, 259`, with a dashed black outline. A thin plus sign centers near `205, 502`, about 39 px across. A 20 px-cap label sits below near `112, 660`. The remaining page is blank white, with More selected in the bottom navigation. The sort label has about 25 px italic caps; no notebook rows or scrollbar are present.

Controls and flow. Close the stylus introduction or open the notebook destination to reach this visible state. The tile starts creation, sort changes order and overflow offers list options not captured. Back returns to More. The filename does not establish an activity/statistics screen or its layout. Capture that destination separately before specifying Kaia's statistics parity.

## Beta features, `more-beta.png`

Purpose. Expose experimental reading/device functions.

Layout. The shared inset settings dialog has a large heading near `124, 205` and two introductory lines at y=291 and 334, 43 px pitch. Gray section strips occupy `100, 379, 1056, 59`, `100, 561, 1056, 59` and `100, 753, 1056, 49`. Their sans-serif caps are about 20 px. The first section has a web-browser description at x=125, y=473 and a right launch action near x=1027. The large-print checkbox is `928, 651, 38, 38` and empty. The saved-word checkbox near `928, 894` is also empty, beside a four-line explanation beginning y=838. Body caps are about 25 px. Close is at the standard upper-right position; no back arrow or scrollbar appears.

Controls and flow. Open beta features from More. Launch opens a browser; the two unchecked controls enable larger UI type and saving looked-up words. Close returns to More. Kaia should use its existing experiment policy, not copy the explanatory text or automatically add a browser. No enabled large-print or word-list state is captured.

## PDF opening state, `pdf-open.png`

Purpose. Record the first captured fixed-layout state without guessing why it is blank.

Layout. The image is white across the screen except for the book progress track at y=1663 and a roughly 26 px-long, 5 px-high black fill at its left. No status bar, page title, document ink, footer text, spinner, error, slider or menu is visible. There is no font size or line pitch to measure in the blank area.

Controls and flow. The filename suggests a newly opened PDF, but the still cannot distinguish a blank source page from loading or a render failure. The next PDF capture contains document content. Test page turning and menu access on device; do not implement a deliberate blank opening page from this evidence. Exit through the reader menu when shown.

## PDF page at fit scale, `pdf-page2.png`

Purpose. Read a scanned PDF page at a smaller scale.

Layout. The scan has a centered heading near x=512, y=95, with about 44 px caps. The contents entries span roughly x=152 to 1109, y=247 to 1083. Entry pitch is about 57 px, with approximate scanned cap heights of 25 to 28 px. These metrics belong to the document image, not the application font. The rest below is white. There is no app header/footer text or toolbar; the y=1663 progress track has about 52 px of fill. The source page's own words, dots and page numbers are not controls.

Controls and flow. Reach this page after opening the PDF. The sweep notes report pinch zoom around the fingers and drag-to-pan, corroborated by the following changed scale/position captures. Gesture thresholds and exact scale anchoring need device verification. Page taps/buttons and the center-menu route should be tested. Do not apply EPUB default typography to scanned content.

## PDF zoomed after pinch, `pdf-pinch-out.png`

Purpose. Read a magnified crop of the same scanned contents page.

Layout. The document fills and clips at all screen edges. About ten rows are visible at roughly 173 px pitch, approximately three times the fit-scale pitch. Scanned capitals are roughly 72 to 85 px high, depending on glyph and print irregularity. Text at the left is clipped and the right page-number column is outside the view. No toolbar, overview, zoom number or persistent footer text is visible. The 1 px progress track and roughly 52 px fill remain near y=1663. Black scan ink sits on white.

Controls and flow. The supplied filename records a pinch-out attempt and the image shows a larger crop. The later zoom menu explicitly shows 300 percent on another page. Do not infer the exact pinch-to-scale curve from the threefold pitch alone. Test pinch anchoring, panning, turning and menu dismissal at this scale. The image has no extra visible control to close zoom directly.

## PDF zoomed and panned, `pdf-zoom-pan.png`

Purpose. Inspect a different horizontal portion of the magnified page.

Layout. Text and dot rows remain at about 173 px pitch and the same scan size as `pdf-pinch-out.png`, but almost all left-side words have moved beyond the left edge. The screen is mostly dotted leader lines, reaching about x=1238. Y positions remain broadly the same. No scrollbar, overview, title bar or footer label appears; the progress track remains at y=1663 with the same fill. This is a viewport translation, not a text reflow.

Controls and flow. The filename records a pan attempt, and the sweep notes report that dragging pans. The visible shift supports a changed viewport, but does not give the gesture direction, distance ratio or edge constraints. Test drag-to-pan and the distinction between panning and page turns. Open the center menu for navigation or use the verified page-turn action. No separate pan-exit control is visible in this state.

## PDF right-tap result, `pdf-zoom-tap-right.png`

Purpose. Record the result after a named right-side tap while zoomed.

Layout. The scan content has changed from a contents page to a dense blackletter page. About 13 partial/full lines fit with approximately 135 px pitch; scan capitals vary around 75 to 95 px. Both horizontal edges and the first/last lines are clipped. No application toolbar, overview or footer label appears. The progress fill is now about 77 px long, still at y=1661 to 1665 on the y=1663 track. This supports a page change rather than only a pan; the next menu identifies page 3 of 45 at 300 percent.

Controls and flow. The sweep notes explicitly report that a right tap turns to the next PDF page at the same zoom and offset, rather than advancing across the current page. The still and following menu corroborate the page/zoom result. Verify left taps, hardware turns and offset clamping on differently sized pages. The source scan's printed page number is distinct from the PDF's document-page index.

## PDF zoom menu, `pdf-zoom-menu.png`

Purpose. Show reader tools and a page overview at a magnified PDF scale.

Layout. The status row and y=178 toolbar rule match the EPUB menu. The left label returns to Search, not Home, because this document was opened from a result. Right tools center near x=740 for pan/zoom, 831 for pen, 923 for orientation, 1013 for light, 1104 for settings and 1199 for More. The PDF has no font tool. The bottom menu begins at y=1344, with a centered 3-of-45 position label, slider y=1512 and thumb near x=196. The lower-left text is the document title; annotations and search stay at the right. Edge-navigation icons are pale gray in this capture.

An overview card at `42, 294, 303, 575` has a 1 px black border. Its page preview is `67, 319, 253, 422`, with a rectangle marking the visible area. Two sans-serif status lines begin near `67, 775` and `67, 815`, about 23 px caps and 40 px pitch, showing document page 3 of 45 and zoom 300 percent. The magnified scan stays behind the bars and overview.

Controls and flow. Open the reader menu while zoomed. Return goes back to search results. The pan/zoom icon opens scale controls, while pen, rotation, light, settings, More, contents, annotations and search retain their named purposes. The slider seeks document pages. The overview may support viewport repositioning, but only its display is proven here. Verify interaction with its viewport box before implementing dragging.

## PDF pan/zoom tool, `pdf-move-tool.png`

Purpose. Adjust PDF magnification from the toolbar.

Layout. The pan/zoom icon at x=740 receives a black 80 px disk. A full-width white strip appears from y=179 to 317, with a 1 px black lower rule. Minus sits near `68, 247`, plus near `1195, 247`. The track runs x=131 to about 1111 at y=248, with a 5 px black fill and a 48 px ring knob near x=1111. The overview card stays at `42, 294, 303, 575`, overlapping the strip's lower edge. It still reports page 3 of 45 and 300 percent. The usual lower navigation panel is absent, so the scan remains visible down to the progress track.

Controls and flow. Tap the PDF pan/zoom tool from the menu. Use minus/plus or drag the knob to change scale. The tool's name and icon suggest movement as well, but the capture proves only the scale strip and overview. Verify viewport drag, scale range and whether tapping the icon/page closes the strip. No font-family, line-spacing or margin controls apply to this fixed-layout document.

## PDF reduced after pinch, `pdf-pinch-in.png`

Purpose. Return to a smaller view of the current scanned page.

Layout. The blackletter page is now largely visible. Its own printed page marker sits near x=625, y=0 to 26 with an underline near y=45. Main scan ink spans roughly x=182 to 1105, from y=92 to 1580. About 33 body lines fit at roughly 45 px pitch; scanned cap height varies around 24 to 32 px. No toolbar, overview or footer label remains. The progress track at y=1663 retains about 77 px of fill. The page's own printed number is not the reader's third-page indicator.

Controls and flow. The filename records a pinch-in attempt and the smaller scan corroborates a scale decrease. The exact resulting zoom percentage is not visible. Test whether this gesture returns to fit-page or merely a lower manual scale. Page turns and menu access remain necessary; the next screenshot shows the menu at this smaller scale.

## PDF menu at reduced scale, `pdf-menu.png`

Purpose. Navigate the PDF after zooming back out.

Layout. The top status/actions and lower navigation match `pdf-zoom-menu.png`, including return-to-Search, the PDF pan/zoom icon and page 3 of 45. The full-scale overview card is now absent. The scan remains at the same coordinates as `pdf-pinch-in.png` and is covered by the toolbar above y=178 and lower panel below y=1344. The slider thumb is near x=196 on the y=1512 track. Toolbar glyphs are about 44 to 52 px; page-position caps about 21 px; the document-title label about 25 px. Palette is black on white with gray disabled edge-navigation icons.

Controls and flow. Open the menu at the reduced scale. The PDF tool set, slider and contents/annotations/search controls apply. Return must restore the search-results context. Tap the exposed page/menu zone to dismiss, subject to device confirmation. The absence of an overview here suggests scale-dependent visibility, but the exact zoom threshold is not measured.
