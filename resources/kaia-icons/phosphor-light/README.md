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
| `align.justify` | `text-align-justify-light` | Justified text alignment |
| `align.left` | `text-align-left-light` | Left text alignment |
| `appbar.menu` | `dots-three-light` | More, reader overlay |
| `appbar.navigation` | `list-bullets-light` | Contents, reader overlay |
| `appbar.pagefit` | `arrows-out-simple-light` | Expand dictionary |
| `appbar.rotation` | `device-rotate-light` | Screen orientation |
| `appbar.search` | `magnifying-glass-light` | Search |
| `appbar.settings` | `gear-six-light` | Settings |
| `appbar.tools` | `flask-light` | Experiments |
| `appbar.typeset` | `text-aa-light` | Text and layout, reader overlay |
| `book.opened` | `books-light` | Library, nav bar |
| `bookmark` | `bookmark-simple-light` | Notes and reader bookmarks |
| `check` | `check-light` | Checkbox, list selection |
| `chevron.down` | `caret-down-light` | Next list page and font dropdown |
| `chevron.first` | `caret-double-left-light` | Previous reading location |
| `chevron.last` | `caret-double-right-light` | Next reading location |
| `chevron.left` | `arrow-left-light` | Back, header and reader overlay; trailing row mark in RTL |
| `chevron.right` | `caret-right-light` | Trailing row mark; back in RTL |
| `chevron.up` | `caret-up-light` | Previous list page |
| `close` | `x-light` | Close panel and sheet |
| `edit` | `note-pencil-light` | Notes, reader overlay |
| `home` | `house-light` | Home, reader overlay |
| `light.moon` | `moon-light` | Nighttime warmth |
| `light.sun` | `sun-light` | Brightness and daytime warmth |
| `more` | `dots-three-light` | Reader More |
| `move` | `arrows-out-cardinal-light` | PDF move and zoom |
| `notice-question` | `question-light` | Help |
| `settings.back` | `arrow-left-light` | Settings back |
| `settings.back.rtl` | `arrow-right-light` | Settings back in RTL |
| `settings.down` | `caret-down-light` | Settings dropdown |
| `star.empty` | `star-light` | Not a favourite, library row |
| `star.full` | `star-fill` | Favourite, library row |
| `sun` | `sun-light` | Reader light |
| `wifi` | `wifi-high-light` | Wi-Fi status |
| `wifi.open.100` | `wifi-high-light` | Open network signal |
| `wifi.open.25` | `wifi-low-light` | Open network signal |
| `wifi.open.50` | `wifi-medium-light` | Open network signal |
| `wifi.secure.100` | `wifi-high-light` | Secure network signal |
| `wifi.secure.25` | `wifi-low-light` | Secure network signal |
| `wifi.secure.50` | `wifi-medium-light` | Secure network signal |

Names in `resources/icons/mdlight` that the Rust app does not pass to an icon
component stay in that folder, and the fallback still serves them.
