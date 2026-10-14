//! What each page turn choice becomes on the Kobo panel: the waveform, the flash and
//! the waits. The native backend is the default. The FBInk plan is kept for comparison.
//! Both run on the host, because the plans have no device access.

#![cfg(feature = "kobo")]

use kaia_display::kobo::native::abi::{
    HWTCON_WAVEFORM_MODE_DU, HWTCON_WAVEFORM_MODE_GLR16, UPDATE_MODE_FULL, UPDATE_MODE_PARTIAL,
};
use kaia_display::kobo::native::geometry::Orientation;
use kaia_display::kobo::native::update::{request_for, Sequencer, Step};
use kaia_display::kobo::plan::plan_refresh;
use kaia_display::waveform::{PageTurn, Refresh};
use kaia_display::Rotation;
use kaia_fbink_sys as fb;
use kaia_gfx::{Rect, Size};

const SCREEN: Size = Size::new(1264, 1680);

/// The FBInk plan for a full-screen partial page turn with `choice`.
fn fbink_plan(choice: PageTurn) -> kaia_display::kobo::plan::RefreshPlan {
    let refresh = Refresh::new(Rect::new(0, 0, 1264, 1680), choice.waveform(), false);
    plan_refresh(&refresh, SCREEN)
}

/// The native steps for a full-screen partial page turn with `choice`, from a fresh
/// sequencer, as the first update after the app starts.
fn native_steps(choice: PageTurn) -> Vec<Step> {
    let orientation = Orientation::new(SCREEN, Rotation::Deg0);
    let request = request_for(choice.waveform(), false, false);
    Sequencer::default()
        .plan_update(&orientation, Rect::new(0, 0, 1264, 1680), request)
        .expect("a full-screen update is not discarded")
}

#[test]
fn cleaner_page_turn_is_reagl_as_koreader_sends_it() {
    let plan = fbink_plan(PageTurn::Cleaner);
    assert_eq!(plan.waveform, fb::WFM_REAGL);
    assert!(plan.flashing, "REAGL runs in FULL update mode");
    assert!(plan.wait_before && plan.wait_after);
}

#[test]
fn faster_page_turn_is_fast_black_and_white_without_flash_or_waits() {
    let plan = fbink_plan(PageTurn::Faster);
    assert_eq!(plan.waveform, fb::WFM_DU);
    assert!(!plan.flashing);
    assert!(!plan.wait_before && !plan.wait_after && !plan.wait_submission_after);
    assert!(!plan.color_post, "DU never takes the CFA post-process");
}

#[test]
fn native_cleaner_page_turn_is_a_full_reagl_update_with_a_fence() {
    let steps = native_steps(PageTurn::Cleaner);
    match steps.as_slice() {
        [Step::Poweron, Step::Send(data), Step::WaitComplete(_)] => {
            assert_eq!(data.waveform_mode, HWTCON_WAVEFORM_MODE_GLR16);
            assert_eq!(data.update_mode, UPDATE_MODE_FULL);
        }
        other => panic!("unexpected steps for the cleaner page turn: {other:?}"),
    }
}

#[test]
fn native_faster_page_turn_is_a_partial_du_update_without_waits() {
    let steps = native_steps(PageTurn::Faster);
    match steps.as_slice() {
        [Step::Poweron, Step::Send(data)] => {
            assert_eq!(data.waveform_mode, HWTCON_WAVEFORM_MODE_DU);
            assert_eq!(data.update_mode, UPDATE_MODE_PARTIAL);
        }
        other => panic!("unexpected steps for the faster page turn: {other:?}"),
    }
}
