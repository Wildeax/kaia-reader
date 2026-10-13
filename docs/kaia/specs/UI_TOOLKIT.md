# kaia-ui toolkit (R0, R2)

Status: proposed, version 1. The toolkit draws every Kaia surface. It runs on one thread, paints into a `kaia_gfx::Pixmap`, and turns paint requests into E Ink refreshes. It must work headless so tests and agents can check layouts as PNG files.

## Model

A retained tree of widgets. Each window on the `kaia_events::WindowStack` owns one root widget.

```rust
pub trait Widget {
    /// Measure and place children. Must return a size within `c`.
    fn layout(&mut self, cx: &mut LayoutCx, c: Constraints) -> Size;
    /// Draw into `cx.target` inside `cx.bounds()` (set by the parent after layout).
    fn paint(&mut self, cx: &mut PaintCx);
    /// Handle an input or named event. Return Handled to stop propagation.
    fn event(&mut self, cx: &mut EventCx, ev: &UiEvent) -> Handled;
    /// Children in paint order, for hit testing and focus.
    fn children_mut(&mut self) -> Vec<&mut WidgetPod>;
}
```

`WidgetPod` wraps a boxed widget with its laid-out `Rect`, an id, and flags (`needs_layout`, `needs_paint`, `focusable`, `visible`). Parents call `pod.layout(cx, c)` and then `pod.set_origin(point)`.

Constraints are box constraints: `min: Size`, `max: Size`. `Constraints::tight(size)` and `Constraints::loose(size)` exist. Layout is one pass, top-down constraints and bottom-up sizes.

## Events

```rust
pub enum UiEvent {
    Gesture(kaia_input::Gesture),   // tap, hold, swipe, pan, pinch...
    Key(kaia_input::KeyEvent),
    Named(kaia_events::Event),      // app and plugin events by name
    FocusGained, FocusLost,
}
```

- Gestures with a position go to the deepest widget whose bounds contain the position, then bubble up through parents until one returns `Handled::Yes`. Swipes use their start position.
- Keys go to the focused widget and bubble up. Page-turn keys move focus between focusable widgets when no widget handles them (keyboard and button navigation).
- Named events are delivered by the window stack (`send` and `broadcast` semantics in `kaia-events`), then to every widget in the window in paint order, until one handles it.

## Painting and refresh

- A widget calls `cx.request_paint()` for itself or `cx.request_paint_rect(rect, hint)` for part of itself. Hints: `Ui` (default), `Partial`, `Fast`, `Full`, `Flash`. The app collects requests into `kaia_events::dirty::DirtyQueue`, repaints the affected windows bottom-up into the frame pixmap with clipping, and sends `kaia_display::Refresh` values using the mode to waveform table in `kaia-display`.
- No widget animates or repaints on a timer unless the user started an action that needs it (for example a progress spinner during a long load, limited to two frames per second).
- Painting is clipped to the widget bounds and to the dirty region.

## Theme

`Theme` holds the tokens from `docs/kaia/specs/DESIGN.md`: spacing, radii, lines, fonts (family, weight, size), colors and the accent rule (green on color panels, ink on grayscale). Sizes are given in device-independent units and scaled with KOReader's `scaleBySize` rule from `base/ffi/framebuffer.lua`: `ceil(x * (size_scale + dpi_scale) / 2)` with `size_scale = min(w, h) / 600` and `dpi_scale` equal to `size_scale` unless a DPI override is set. DESIGN.md values therefore carry over (`dp(50)` is 106 px on the Libra Colour). Widgets never hardcode sizes or colors.

## Components (version 1)

Layout: `Column`, `Row` (with `Expanded` and `Spacer` children and main and cross axis alignment), `Stack`, `Padding`, `Align`, `SizedBox`, `Frame` (background, border, radius, padding).

Content: `Label` (one line, ellipsis), `Paragraph` (wrapped, max lines, ellipsis, RTL), `Icon` (SVG from `resources/icons`), `Image` (pixmap, fit contain or cover), `Divider`, `ProgressBar` (with percent text so the value never depends on color), `Cover` (book cover with a placeholder).

Controls: `Button` (text, icon, or both; primary, secondary and text styles; disabled state), `IconButton`, `Toggle`, `Checkbox`, `RadioGroup`, `Stepper` (minus and plus with a value, for font size and margins), `Slider` with discrete steps, `TextField` (single line, caret, selection by tap) and `OnScreenKeyboard` (QWERTY with shift, symbols and number layers, language layouts later).

Composite: `ListRow` (primary and secondary text, optional leading icon or cover, trailing chevron or toggle), `PagedList` (pages instead of scrolling, with a page indicator and previous and next buttons; E Ink must not scroll), `Header` (title, back action, right actions), `NavBar` (Home, Library, Notes, Settings), `Dialog` (modal window with title, body, and up to three buttons), `Menu` (tabbed, paged menu built from a `kaia_events` menu registry), `Toast` (short message window that closes itself after a delay), `SheetPanel` (a panel from the bottom edge, used for reading settings).

Foreign: `ForeignWidget` hands its rect and events to a `trait ForeignRenderer { fn paint(&mut self, target: &mut Pixmap, rect: Rect); fn event(&mut self, ev: &UiEvent) -> Handled; }`. Phase R7 uses it for legacy Lua widgets.

## Testing

- `render(root, size, theme) -> Pixmap` lays out and paints a tree headless.
- `Harness` wraps a window stack and a headless display: `tap(point)`, `swipe(from, to)`, `key(k)`, `send(event)`, `frame()`, `refreshes()`.
- Every component has a golden PNG test at 1264x1680 and 300 DPI, plus behavior tests through the harness.

## App loop contract

Each loop iteration runs in this order: read input, dispatch it to the window stack, paint dirty windows and send refreshes, then run the scheduler's due tasks, then paint again if a task made something dirty. Press feedback depends on this order: a tapped button paints its pressed state in the first paint and releases in a `next_tick` task, so the pressed frame reaches the panel before the release.

Controls clear their own bounds to the background tone they inherit from the nearest `Frame` (through `Widget::child_background`), or to a tone set with `.background(...)`; the default is the theme surface.
