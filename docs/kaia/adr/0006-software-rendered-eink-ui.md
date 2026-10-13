# ADR-006: Software-rendered E Ink UI through one refresh path

Status: Proposed

Date: 2026-10-13

## Context

KOReader's UIManager already handles refresh regions, refresh modes, scheduling and widget ordering. [S25] The plan asks Kaia to keep these semantics before it tries to optimize or replace them.

The Libra Colour has a 1264 by 1680 display. Kobo lists 300 PPI for monochrome content and 150 PPI for color content. [S26] E Ink panels show ghosting, and the image quality of drawings and stickers needs its own tests. Desktop screenshots do not show these effects. The panel must be tested on the device.

Continuous animation, blinking indicators and frequent clock updates cause extra refreshes. The plan avoids them.

## Decision

Kaia builds its UI with KOReader's widget, text and display system. The refresh path stays UIManager's path. Rust-backed components, added later, draw into the same composition path. The first release does not include a browser, a GPU-dependent framework or an independent compositor.

The UI is grayscale first. Color is an accent. Color never works as the only indicator of state.

Kaia redraws only changed regions. The existing refresh backend picks the refresh modes. The first release does not tune those modes.

Kaia targets five product surfaces: Home, Library, Reading, Notes and Settings. It keeps reading controls reachable. It groups advanced settings instead of removing them.

## Consequences

- Visual design is limited to what the existing widget and display system can draw.
- Ghosting and image quality need tests on the physical panel.
- Legacy plugin dialogs must stay usable and correctly layered. This is the second visual gate in ENGINEERING_PLAN.md section 6.
- Redrawing only changed regions keeps idle work low. It also needs tests that catch missed redraws.

## Evidence needed to accept

- KR-010 keeps one event and display owner. The hybrid proof shows no incorrect layering, no lost input and no competing redraw loop. The proof measures its performance.
- KR-013 gives dialogs, buttons, menus and settings one design system. Touch and page-turn buttons, large text, right-to-left layouts and localization pass the tests.
- The physical Libra Colour panel passes the ghosting and image quality checks.
- The idle gate in ENGINEERING_PLAN.md section 8 holds. The device shows no unexplained redraws and no new periodic work without a feature justification.
- The Phase 2 exit gate requires daily reading to work without data regression. Shared plugin dialogs stay usable.

## Sources

- S25: UIManager. https://github.com/koreader/koreader/blob/master/frontend/ui/uimanager.lua
- S26: Official Libra Colour specifications. https://ca.kobobooks.com/products/kobo-libra-colour
