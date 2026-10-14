//! Libra Colour mixer selection against a fake device tree: AW99703 when its
//! control file exists, LM3630 otherwise. Each test checks where warmth goes.

mod common;

use common::{get, put, FakeIo};
use kaia_platform::frontlight::{
    Frontlight, FrontlightConfig, MixerNode, SysfsFrontlight, AW99703_COLOR, LM3630_COLOR,
};
use kaia_platform::sysfs::Root;

const BRIGHTNESS: &str = "/sys/class/backlight/mxc_msp430.0/brightness";

/// A tree with the white LED brightness and the LM3630 mixer. The AW99703
/// mixer is added only when `with_aw99703` is set.
fn tree(with_aw99703: bool) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    put(dir.path(), BRIGHTNESS, "20\n");
    put(dir.path(), LM3630_COLOR, "5\n");
    if with_aw99703 {
        put(dir.path(), AW99703_COLOR, "5\n");
    }
    dir
}

fn open_with_saved_warmth(dir: &tempfile::TempDir) -> SysfsFrontlight<FakeIo> {
    // Saved warmth 80 is native 8, which the inverted mixer writes as 10 - 8 = 2.
    SysfsFrontlight::open(
        FrontlightConfig::monza(),
        FakeIo::new(dir.path()),
        &Root::new(dir.path()),
        Some(80),
    )
}

#[test]
fn aw99703_present_warmth_goes_to_aw99703() {
    let dir = tree(true);
    let fl = open_with_saved_warmth(&dir);

    assert_eq!(fl.config().mixer_node, Some(MixerNode::Aw99703));
    assert_eq!(fl.config().mixer_path, AW99703_COLOR);
    assert!(
        !fl.config().delay_ramp_start,
        "AW99703 does not defer the ramp start"
    );
    assert_eq!(fl.warmth(), 80);
    assert_eq!(
        fl.writer().writes,
        vec![(AW99703_COLOR.to_string(), "2".to_string())],
        "the only write at open is the warmth, and it goes to AW99703"
    );
    assert_eq!(get(dir.path(), AW99703_COLOR), "2");
    assert_eq!(
        get(dir.path(), LM3630_COLOR),
        "5\n",
        "the LM3630 mixer is not written"
    );
}

#[test]
fn only_lm3630_present_warmth_goes_to_lm3630() {
    let dir = tree(false);
    let fl = open_with_saved_warmth(&dir);

    assert_eq!(fl.config().mixer_node, Some(MixerNode::Lm3630));
    assert_eq!(fl.config().mixer_path, LM3630_COLOR);
    assert!(
        fl.config().delay_ramp_start,
        "LM3630 on MTK defers the ramp start"
    );
    assert_eq!(
        fl.writer().writes,
        vec![(LM3630_COLOR.to_string(), "2".to_string())],
        "the only write at open is the warmth, and it goes to LM3630"
    );
    assert_eq!(get(dir.path(), LM3630_COLOR), "2");
    assert!(!dir.path().join(AW99703_COLOR.trim_start_matches('/')).exists());
}

#[test]
fn neither_mixer_present_falls_back_to_lm3630() {
    // No colour control file at all. The rule falls back to LM3630, so the
    // warmth write is attempted there. The write fails and the value stays in memory.
    let dir = tempfile::tempdir().unwrap();
    put(dir.path(), BRIGHTNESS, "20\n");
    let fl = open_with_saved_warmth(&dir);

    assert_eq!(fl.config().mixer_node, Some(MixerNode::Lm3630));
    assert_eq!(
        fl.writer().writes,
        vec![(LM3630_COLOR.to_string(), "2".to_string())]
    );
    assert_eq!(fl.warmth(), 80);
}
