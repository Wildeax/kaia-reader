//! Refresh modes and the refresh queue, mirroring `UIManager:setDirty`,
//! `UIManager:_refresh` and the refresh part of `UIManager:_repaint`
//! (`frontend/ui/uimanager.lua`).

use std::collections::BTreeSet;
use std::rc::Rc;

use crate::geom::Rect;
use crate::settings::Settings;
use crate::window::WindowId;

/// How the panel refreshes a region. Names are KOReader's.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RefreshMode {
    /// `a2`: low fidelity, black and white to black and white only.
    A2,
    /// `fast`: low fidelity, for inversion effects.
    Fast,
    /// `ui`: medium fidelity. The default for most UI.
    Ui,
    /// `partial`: medium fidelity for text on white. Counts toward flash promotion.
    Partial,
    /// `[ui]`: `ui` that the driver must not merge with neighbours.
    NoMergeUi,
    /// `[partial]`: `partial` that the driver must not merge with neighbours.
    NoMergePartial,
    /// `flashui`: `ui` with a flash.
    FlashUi,
    /// `flashpartial`: `partial` with a flash. Does not count toward promotion.
    FlashPartial,
    /// `full`: high fidelity flashing refresh.
    Full,
}

impl RefreshMode {
    pub const ALL: [RefreshMode; 9] = [
        RefreshMode::A2,
        RefreshMode::Fast,
        RefreshMode::Ui,
        RefreshMode::Partial,
        RefreshMode::NoMergeUi,
        RefreshMode::NoMergePartial,
        RefreshMode::FlashUi,
        RefreshMode::FlashPartial,
        RefreshMode::Full,
    ];

    /// The name KOReader uses in `setDirty` calls.
    pub fn name(self) -> &'static str {
        match self {
            RefreshMode::A2 => "a2",
            RefreshMode::Fast => "fast",
            RefreshMode::Ui => "ui",
            RefreshMode::Partial => "partial",
            RefreshMode::NoMergeUi => "[ui]",
            RefreshMode::NoMergePartial => "[partial]",
            RefreshMode::FlashUi => "flashui",
            RefreshMode::FlashPartial => "flashpartial",
            RefreshMode::Full => "full",
        }
    }

    pub fn from_name(name: &str) -> Option<RefreshMode> {
        RefreshMode::ALL.into_iter().find(|mode| mode.name() == name)
    }

    /// The precedence from `refresh_modes` in `UIManager`. When two queued
    /// refreshes merge, the mode with the higher rank wins.
    pub fn rank(self) -> u8 {
        match self {
            RefreshMode::A2 => 1,
            RefreshMode::Fast => 2,
            RefreshMode::Ui => 3,
            RefreshMode::Partial => 4,
            RefreshMode::NoMergeUi => 5,
            RefreshMode::NoMergePartial => 6,
            RefreshMode::FlashUi => 7,
            RefreshMode::FlashPartial => 8,
            RefreshMode::Full => 9,
        }
    }
}

/// One refresh request: a mode, a region (`None` means the whole screen)
/// and a dithering hint.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Refresh {
    pub mode: RefreshMode,
    pub region: Option<Rect>,
    pub dither: bool,
}

impl Refresh {
    pub fn new(mode: RefreshMode) -> Self {
        Self {
            mode,
            region: None,
            dither: false,
        }
    }

    pub fn region(mode: RefreshMode, region: Rect) -> Self {
        Self {
            mode,
            region: Some(region),
            dither: false,
        }
    }
}

/// Collects repaint and refresh requests for one frame.
///
/// `refresh` merges overlapping requests the way `UIManager:_refresh` does.
/// `take_frame` returns the list that `_repaint` would hand to the panel.
#[derive(Debug, Clone)]
pub struct DirtyQueue {
    screen: Rect,
    queue: Vec<Refresh>,
    marked: BTreeSet<WindowId>,
    deferred: Vec<DeferredRefresh>,
    painted_dithered: bool,
    refresh_count: i64,
    refresh_counted: bool,
    /// `FULL_REFRESH_COUNT`. Zero or less disables promotion of partial refreshes.
    pub full_refresh_count: i64,
    /// `avoid_flashing_ui` setting: downgrades flashing modes.
    pub avoid_flashing_ui: bool,
    /// Set by reader scrolling: every refresh becomes `fast`.
    pub currently_scrolling: bool,
    /// Whether the panel supports hardware dithering. Without it, dither hints are dropped.
    pub hw_dithering: bool,
}

#[derive(Clone)]
struct DeferredRefresh(Rc<dyn Fn() -> Option<Refresh>>);

impl std::fmt::Debug for DeferredRefresh {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("DeferredRefresh")
    }
}

/// `DEFAULT_FULL_REFRESH_COUNT` in `uimanager.lua`.
pub const DEFAULT_FULL_REFRESH_COUNT: i64 = 6;

impl DirtyQueue {
    /// A queue for a screen of the given size, with KOReader's defaults.
    pub fn new(screen: Rect) -> Self {
        Self {
            screen,
            queue: Vec::new(),
            marked: BTreeSet::new(),
            deferred: Vec::new(),
            painted_dithered: false,
            refresh_count: 0,
            refresh_counted: false,
            full_refresh_count: DEFAULT_FULL_REFRESH_COUNT,
            avoid_flashing_ui: false,
            currently_scrolling: false,
            hw_dithering: false,
        }
    }

    /// Reads the refresh settings the way `UIManager` does at start-up and
    /// in `setRefreshRate`: `night_full_refresh_count` in night mode,
    /// otherwise `full_refresh_count`, both defaulting to 6.
    pub fn configure_from(&mut self, settings: &Settings) {
        let day = settings.read("full_refresh_count").as_i64();
        let night = settings.read("night_full_refresh_count").as_i64();
        self.full_refresh_count = if settings.is_true("night_mode") {
            night.or(day).unwrap_or(DEFAULT_FULL_REFRESH_COUNT)
        } else {
            day.unwrap_or(DEFAULT_FULL_REFRESH_COUNT)
        };
        self.avoid_flashing_ui = settings.is_true("avoid_flashing_ui");
    }

    /// Flags a window for repaint, as `setDirty(widget)` does for a
    /// window-level widget.
    pub fn mark(&mut self, id: WindowId) {
        self.marked.insert(id);
    }

    pub fn unmark(&mut self, id: WindowId) {
        self.marked.remove(&id);
    }

    pub fn is_marked(&self, id: WindowId) -> bool {
        self.marked.contains(&id)
    }

    /// Records a dithered window in the current paint plan.
    pub fn painted_dithered(&mut self) {
        self.painted_dithered = true;
    }

    /// `setDirty(widget, refreshtype, region)`. Marks the window when given,
    /// then enqueues the refresh when one is requested.
    pub fn set_dirty(&mut self, window: Option<WindowId>, refresh: Option<Refresh>) {
        if let Some(id) = window {
            self.mark(id);
        }
        if let Some(refresh) = refresh {
            self.refresh(refresh.mode, refresh.region, refresh.dither);
        }
    }

    /// The function form of `setDirty`. The callback runs in `take_frame`,
    /// after the caller has painted the windows.
    pub fn set_dirty_with(
        &mut self,
        window: Option<WindowId>,
        refresh: impl Fn() -> Option<Refresh> + 'static,
    ) {
        if let Some(id) = window {
            self.mark(id);
        }
        self.deferred.push(DeferredRefresh(Rc::new(refresh)));
    }

    /// The refreshes queued so far, in queue order.
    pub fn queued(&self) -> &[Refresh] {
        &self.queue
    }

    /// `UIManager:_refresh(mode, region, dither)`.
    ///
    /// The steps follow KOReader's order: scrolling downgrade, full-screen
    /// counter reset, `avoid_flashing_ui` downgrade, `partial` promotion, then
    /// merging with every queued refresh whose region overlaps. KOReader
    /// merges by recursing; this loop repeats the same normalisation after
    /// each merge, so the result matches.
    pub fn refresh(&mut self, mode: RefreshMode, region: Option<Rect>, dither: bool) {
        let had_region = region.is_some();
        let mut mode = self.normalise(mode, had_region);
        let mut region = region.unwrap_or(self.screen);
        let mut dither = dither;
        loop {
            let collision = self
                .queue
                .iter()
                .position(|queued| region.open_intersects(&queued.region.unwrap_or(self.screen)));
            let Some(index) = collision else { break };
            let other = self.queue.remove(index);
            let other_region = other.region.unwrap_or(self.screen);
            region = region.union(&other_region);
            mode = if other.mode.rank() > mode.rank() {
                other.mode
            } else {
                mode
            };
            dither = dither || other.dither;
            mode = self.normalise(mode, true);
        }
        self.queue.push(Refresh {
            mode,
            region: Some(region),
            dither,
        });
    }

    /// The per-call normalisation shared by the first call and every merge.
    fn normalise(&mut self, mode: RefreshMode, had_region: bool) -> RefreshMode {
        let mut mode = if self.currently_scrolling {
            RefreshMode::Fast
        } else {
            mode
        };
        if !had_region && mode == RefreshMode::Full {
            self.refresh_count = 0;
        }
        if self.avoid_flashing_ui {
            mode = match mode {
                RefreshMode::FlashUi => RefreshMode::Ui,
                RefreshMode::FlashPartial => RefreshMode::Partial,
                RefreshMode::Partial if had_region => RefreshMode::Ui,
                other => other,
            };
        }
        if mode == RefreshMode::Partial && self.full_refresh_count > 0 && !self.refresh_counted {
            self.refresh_count = (self.refresh_count + 1) % self.full_refresh_count;
            if self.refresh_count == self.full_refresh_count - 1 {
                mode = if had_region {
                    RefreshMode::FlashUi
                } else {
                    RefreshMode::Full
                };
            }
            self.refresh_counted = true;
        }
        mode
    }

    /// The end of `_repaint`: returns the refreshes for this frame and resets
    /// the per-frame state. `painted` says whether any window was repainted.
    /// When windows were painted but nothing was queued, a partial refresh of
    /// the whole screen is added, as KOReader does.
    pub fn take_frame(&mut self, painted: bool) -> Vec<Refresh> {
        for callback in std::mem::take(&mut self.deferred) {
            if let Some(refresh) = callback.0() {
                self.refresh(refresh.mode, refresh.region, refresh.dither);
            }
        }
        if painted && self.queue.is_empty() {
            self.refresh(RefreshMode::Partial, None, false);
        }
        let mut frame = std::mem::take(&mut self.queue);
        if !self.hw_dithering {
            for refresh in &mut frame {
                refresh.dither = false;
            }
        } else if self.painted_dithered {
            for refresh in &mut frame {
                refresh.dither = true;
            }
        }
        self.painted_dithered = false;
        self.refresh_counted = false;
        frame
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn screen() -> Rect {
        Rect::new(0, 0, 600, 800)
    }

    #[test]
    fn names_round_trip() {
        for mode in RefreshMode::ALL {
            assert_eq!(RefreshMode::from_name(mode.name()), Some(mode));
        }
        assert_eq!(
            RefreshMode::from_name("flashpartial"),
            Some(RefreshMode::FlashPartial)
        );
        assert_eq!(RefreshMode::from_name("bogus"), None);
    }

    #[test]
    fn precedence_matches_uimanager() {
        let ranks: Vec<u8> = RefreshMode::ALL.iter().map(|m| m.rank()).collect();
        // The rank table lists the modes in the order a2, fast, ui, partial,
        // [ui], [partial], flashui, flashpartial, full. ALL uses the same order.
        assert_eq!(ranks, vec![1, 2, 3, 4, 5, 6, 7, 8, 9]);
    }

    #[test]
    fn overlapping_refreshes_merge_with_higher_mode() {
        let mut q = DirtyQueue::new(screen());
        q.refresh(RefreshMode::Ui, Some(Rect::new(0, 0, 100, 100)), false);
        q.refresh(RefreshMode::Fast, Some(Rect::new(50, 50, 100, 100)), false);
        assert_eq!(q.queued().len(), 1);
        assert_eq!(q.queued()[0].mode, RefreshMode::Ui);
        assert_eq!(q.queued()[0].region, Some(Rect::new(0, 0, 150, 150)));
    }

    #[test]
    fn disjoint_refreshes_stay_separate() {
        let mut q = DirtyQueue::new(screen());
        q.refresh(RefreshMode::Ui, Some(Rect::new(0, 0, 100, 100)), false);
        q.refresh(RefreshMode::Ui, Some(Rect::new(300, 300, 100, 100)), false);
        assert_eq!(q.queued().len(), 2);
    }

    #[test]
    fn dither_is_viral_across_a_merge() {
        let mut q = DirtyQueue::new(screen());
        q.hw_dithering = true;
        q.refresh(RefreshMode::Ui, Some(Rect::new(0, 0, 100, 100)), true);
        q.refresh(RefreshMode::Ui, Some(Rect::new(10, 10, 10, 10)), false);
        assert!(q.queued()[0].dither);
    }

    #[test]
    fn dither_is_dropped_without_hardware_support() {
        let mut q = DirtyQueue::new(screen());
        q.refresh(RefreshMode::Ui, Some(Rect::new(0, 0, 100, 100)), true);
        let frame = q.take_frame(true);
        assert!(!frame[0].dither);
    }

    #[test]
    fn full_refresh_without_region_resets_counter() {
        let mut q = DirtyQueue::new(screen());
        q.full_refresh_count = 3;
        q.refresh(RefreshMode::Partial, None, false);
        q.take_frame(false);
        q.refresh(RefreshMode::Partial, None, false);
        q.take_frame(false);
        q.refresh(RefreshMode::Full, None, false);
        q.take_frame(false);
        // Counter is reset, so the next partial is not promoted.
        q.refresh(RefreshMode::Partial, None, false);
        assert_eq!(q.queued()[0].mode, RefreshMode::Partial);
    }

    #[test]
    fn every_nth_partial_is_promoted() {
        let mut q = DirtyQueue::new(screen());
        q.full_refresh_count = 3;
        let mut modes = Vec::new();
        for _ in 0..6 {
            q.refresh(RefreshMode::Partial, None, false);
            modes.push(q.take_frame(false)[0].mode);
        }
        // The counter is (n + 1) % 3 and promotion happens at 2 = FULL_REFRESH_COUNT - 1,
        // so the 2nd, 5th, 8th... partial refresh is promoted.
        use RefreshMode::{Full, Partial};
        assert_eq!(modes, vec![Partial, Full, Partial, Partial, Full, Partial]);
    }

    #[test]
    fn partial_with_region_promotes_to_flashui() {
        let mut q = DirtyQueue::new(screen());
        q.full_refresh_count = 1;
        q.refresh(RefreshMode::Partial, Some(Rect::new(0, 0, 10, 10)), false);
        assert_eq!(q.queued()[0].mode, RefreshMode::FlashUi);
    }

    #[test]
    fn promotion_disabled_with_zero_count() {
        let mut q = DirtyQueue::new(screen());
        q.full_refresh_count = 0;
        for _ in 0..10 {
            q.refresh(RefreshMode::Partial, None, false);
            assert_eq!(q.take_frame(false)[0].mode, RefreshMode::Partial);
        }
    }

    #[test]
    fn avoid_flashing_downgrades() {
        let mut q = DirtyQueue::new(screen());
        q.avoid_flashing_ui = true;
        q.full_refresh_count = 0;
        q.refresh(RefreshMode::FlashUi, Some(Rect::new(0, 0, 10, 10)), false);
        assert_eq!(q.queued()[0].mode, RefreshMode::Ui);
        q.take_frame(false);
        q.refresh(RefreshMode::FlashPartial, None, false);
        assert_eq!(q.queued()[0].mode, RefreshMode::Partial);
        q.take_frame(false);
        q.refresh(RefreshMode::Partial, Some(Rect::new(0, 0, 10, 10)), false);
        assert_eq!(q.queued()[0].mode, RefreshMode::Ui);
    }

    #[test]
    fn scrolling_forces_fast_even_after_merge() {
        let mut q = DirtyQueue::new(screen());
        q.refresh(RefreshMode::Ui, Some(Rect::new(0, 0, 100, 100)), false);
        q.currently_scrolling = true;
        q.refresh(RefreshMode::Ui, Some(Rect::new(10, 10, 10, 10)), false);
        assert_eq!(q.queued()[0].mode, RefreshMode::Fast);
    }

    #[test]
    fn painted_frame_without_refresh_gets_full_screen_partial() {
        let mut q = DirtyQueue::new(screen());
        q.full_refresh_count = 0;
        let frame = q.take_frame(true);
        assert_eq!(
            frame,
            vec![Refresh {
                mode: RefreshMode::Partial,
                region: Some(screen()),
                dither: false,
            }]
        );
        assert!(q.take_frame(false).is_empty());
    }

    #[test]
    fn set_dirty_marks_and_refreshes() {
        let mut q = DirtyQueue::new(screen());
        let id = WindowId::from_raw(7);
        q.set_dirty(Some(id), Some(Refresh::new(RefreshMode::Ui)));
        assert!(q.is_marked(id));
        assert_eq!(q.queued().len(), 1);
    }

    #[test]
    fn deferred_refresh_runs_after_paint() {
        let mut q = DirtyQueue::new(screen());
        let painted = Rc::new(std::cell::Cell::new(false));
        let callback_painted = Rc::clone(&painted);
        q.set_dirty_with(None, move || {
            assert!(callback_painted.get());
            Some(Refresh::region(RefreshMode::Ui, Rect::new(1, 2, 3, 4)))
        });
        assert!(q.queued().is_empty());
        painted.set(true);
        assert_eq!(q.take_frame(false)[0].region, Some(Rect::new(1, 2, 3, 4)));
    }
}
