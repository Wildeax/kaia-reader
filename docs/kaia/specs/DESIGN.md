# Kaia design system and core surfaces (KR-013, KR-014)

Status: proposed, version 1. Target panel: Kobo Libra Colour, 1264 x 1680, 300 PPI grayscale and 150 PPI color. Desktop screenshots are for review only; the physical panel decides.

## Principles

1. Grayscale first. Color is an accent on color panels and never the only sign of state. Every colored element also differs in shape, weight or label.
2. Few surfaces. Home, Library, Reading, Notes and Settings. Each surface has one obvious way back to Home.
3. No motion. No animations, no blinking, no clock that redraws on its own. Redraw only the region that changed.
4. Large targets. Minimum touch target 9 mm, which is about 106 px at 300 PPI. Use `Screen:scaleBySize` for every size so other DPIs work.
5. Advanced features stay, behind the existing menus. Kaia changes where things are reached, not whether they exist.

## Tokens

All tokens live in `frontend/kaia/theme.lua`. Widgets read tokens; they never hardcode sizes or grays.

### Spacing

Values are passed through `Screen:scaleBySize`.

| Token | Value |
|---|---|
| `space.xs` | 4 |
| `space.sm` | 8 |
| `space.md` | 16 |
| `space.lg` | 24 |
| `space.xl` | 40 |

### Radius and lines

| Token | Value |
|---|---|
| `radius.card` | 12 |
| `radius.button` | 10 |
| `line.hairline` | 1 |
| `line.divider` | 1 |
| `line.focus` | 3 |
| `line.progress` | 6 |

### Type

Fonts are the bundled Noto families, so no new font licenses enter the product.

| Token | Face | Size |
|---|---|---|
| `font.display` | NotoSerif-Bold | 34 |
| `font.title` | NotoSans-Bold | 26 |
| `font.book_title` | NotoSerif-Bold | 22 |
| `font.body` | NotoSans-Regular | 20 |
| `font.body_bold` | NotoSans-Bold | 20 |
| `font.caption` | NotoSans-Regular | 16 |
| `font.label` | NotoSans-Bold | 15 |

Sizes go through `Font:getFace(face, size)`, which already applies the user's UI scaling.

### Color

Grays use `Blitbuffer.Color8`. The accent uses `Blitbuffer.ColorRGB32` only when `Screen:isColorEnabled()` is true; otherwise it falls back to `ink`.

| Token | Gray value | Use |
|---|---|---|
| `color.ink` | 0x00 | Text, icons, primary outlines |
| `color.ink_secondary` | 0x55 | Secondary text, authors, captions |
| `color.ink_muted` | 0x88 | Disabled text, placeholders |
| `color.divider` | 0xCC | Dividers and card borders |
| `color.surface` | 0xFF | Page background |
| `color.surface_alt` | 0xEE | Cards, selected rows |
| `color.accent` | RGB 0x1F 0x6F 0x5C on color panels, else ink | Progress fill, active nav item underline |

## Components

| Component | Module | Notes |
|---|---|---|
| Nav bar | `frontend/kaia/widget/navbar.lua` | Bottom bar with Home, Library, Notes, Settings. Icon plus label. The active item has a bold label and a `line.focus` underline. Height 120 px at 300 PPI scaled. |
| Header | `frontend/kaia/widget/header.lua` | Surface title in `font.title` on the left, optional right action. Static battery level text, drawn only when the surface repaints. |
| Book card | `frontend/kaia/widget/bookcard.lua` | Cover, title in `font.book_title`, author in `font.caption` and `ink_secondary`, progress bar and percent. Two sizes: `hero` (Continue reading) and `shelf` (recent row). Missing cover shows a framed placeholder with the title. |
| Progress bar | `frontend/kaia/widget/progressbar.lua` | `line.progress` tall, `divider` track, `accent` fill, percent text beside it so the value never depends on color. |
| List row | `frontend/kaia/widget/listrow.lua` | Two-line row for Notes: primary text and secondary text, `space.md` padding, hairline divider. |

Kaia components are new Lua modules built from existing KOReader widgets (`FrameContainer`, `VerticalGroup`, `HorizontalGroup`, `TextWidget`, `TextBoxWidget`, `ImageWidget`, `IconWidget`, `InputContainer`, `GestureRange`). They paint through the existing `UIManager` refresh path. They do not start timers.

## Shared KOReader components

The first release keeps KOReader dialogs and menus working unchanged and restyles them only through `ui/size.lua` values: window and button radius, dialog padding. Each such change must keep plugin dialogs readable and dismissible. A later change can restyle more once screenshots of the bundled plugins confirm nothing breaks.

## Surfaces

### Home

```text
+--------------------------------------------+
| Kaia                               84%  [=] |  header
|                                            |
|  Continue reading                          |  font.label, ink_secondary
|  +--------+                                |
|  |        |  The Left Hand of Darkness     |  hero book card
|  | cover  |  Ursula K. Le Guin             |
|  |        |  [=========-------]  42%       |
|  +--------+  Chapter 7                     |
|                                            |
|  Recent                                    |
|  [cover] [cover] [cover] [cover]           |  shelf, up to 4 more books
|  title   title   title   title             |
|                                            |
|  Library: 128 books   Notes: 36            |  counts, tap to open
|                                            |
| [Home]   [Library]   [Notes]   [Settings]  |  nav bar
+--------------------------------------------+
```

Data comes from `ReadHistory` (order and last file), `BookInfoManager` from the coverbrowser plugin when it is enabled (covers, title, authors), and `DocSettings` (`percent_finished`, `summary.status`). Home never opens a document to get metadata; missing data shows the file name and a placeholder.

With no history, Home shows "No books yet" and a button to open the Library.

### Library

The existing file manager, with the coverbrowser mosaic view as Kaia's default display mode. Kaia adds the nav bar below the file chooser. The file manager keeps all its menus.

### Reading

The existing reader. Closing a book returns to Home when Kaia Home is the start screen.

### Notes

A list of every book in history that has highlights, notes or Kaia stickers, newest first, then a per-book list of entries in reading order. Tapping an entry opens the book at that place. Orphaned stickers appear in a "Needs repair" group at the top. Highlights and notes are read from each book's `DocSettings` `annotations` table without writing to it.

### Settings

The existing menu, opened from the nav bar.

## Accessibility and localization

- All strings use gettext.
- Layout mirrors for right-to-left languages using `BD.mirroredUILayout()`, as KOReader widgets already do.
- Large UI font sizes must not clip: titles wrap to two lines and truncate with an ellipsis after that.
- Page-turn buttons move focus between nav bar items on devices with keys (`FocusManager`).
