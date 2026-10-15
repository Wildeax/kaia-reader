# Kaia icons (Phosphor Light)

These icons draw the Rust app's controls. The Rust app loads a name from this
folder first. A name missing here falls back to `resources/icons/mdlight`.

## Source

- Project: Phosphor Icons, https://github.com/phosphor-icons/core
- Package: `@phosphor-icons/core` on npm, version 2.1.1
- Archive: `phosphor-icons-core-2.1.1.tgz`, from `npm pack`
- Archive SHA-256: `313332be6190b724da24107addd781799b48bf76b13963f24501112ffe1baadd`
- License: MIT, copied to `LICENSE` in this folder
- Files: `assets/light/*-light.svg`, except `star.full.svg`, which comes from
  `assets/fill/star-fill.svg`. Phosphor has no filled star in the Light weight.
  The filled star marks a favourite, so its shape must differ from the outline.

The SVG files are byte-for-byte copies of the archive files. The product does
not download anything. The copies are made by hand at build time.

## Name map

The left column is the icon name the Rust app passes to the icon component.
The right column is the Phosphor file it came from.

| Kaia name | Phosphor file | Use in Kaia |
|---|---|---|
| `appbar.contrast` | `sun-light` | Light, reader overlay |
| `appbar.menu` | `dots-three-light` | More, reader overlay |
| `appbar.navigation` | `list-bullets-light` | Contents, reader overlay |
| `appbar.search` | `magnifying-glass-light` | Search |
| `appbar.settings` | `gear-six-light` | Settings, home header and nav bar |
| `appbar.typeset` | `text-aa-light` | Text and layout, reader overlay |
| `book.opened` | `books-light` | Library, nav bar |
| `bookmark` | `bookmark-simple-light` | Notes, nav bar; bookmark, reader overlay |
| `check` | `check-light` | Checkbox, list selection |
| `chevron.left` | `arrow-left-light` | Back, header and reader overlay; trailing row mark in RTL |
| `chevron.right` | `caret-right-light` | Trailing row mark; back in RTL |
| `close` | `x-light` | Close panel and sheet |
| `edit` | `note-pencil-light` | Notes, reader overlay |
| `home` | `house-light` | Home, reader overlay |
| `star.empty` | `star-light` | Not a favourite, library row |
| `star.full` | `star-fill` (Fill weight) | Favourite, library row |

Names in `resources/icons/mdlight` that the Rust app does not pass to an icon
component stay in that folder, and the fallback still serves them.
