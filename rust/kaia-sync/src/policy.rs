//! Conflict rules for a remote reading position. Pure functions, no I/O.
//!
//! Each branch names the plugin code it mirrors. Line numbers refer to
//! `plugins/kosync.koplugin/main.lua` unless noted.

use crate::doc_id::round_percent;
use crate::model::Progress;

/// What to do with a remote position.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decision {
    /// Leave the reader where it is. Nothing is sent for this decision.
    Keep,
    /// Send the local position to the server now.
    PushLocal,
    /// Move the reader to the remote position.
    PullRemote,
    /// Ask the user whether to move to the remote position.
    AskUser,
}

/// How a remote position in one direction is handled.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Strategy {
    /// Ask first (`SYNC_STRATEGY.PROMPT`).
    Prompt,
    /// Move without asking (`SYNC_STRATEGY.SILENT`).
    Silent,
    /// Never move to the remote position (`SYNC_STRATEGY.DISABLE`).
    Never,
}

/// The sync settings. Defaults match `KOSync.default_settings`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Policy {
    /// Pull on open, resume and network connect, and push on close. Off by
    /// default. Automatic pulls only run when this is on.
    pub auto_sync: bool,
    /// Strategy for a remote position that is newer ("Sync to a newer state").
    /// With [`Strategy::Never`] the reader stays on the local position.
    pub sync_forward: Strategy,
    /// Strategy for a remote position that is older ("Sync to an older state").
    /// With [`Strategy::Never`] here the reader moves forward only: it follows
    /// newer remote positions and never older ones ("only forward").
    pub sync_backward: Strategy,
}

impl Default for Policy {
    fn default() -> Self {
        Self {
            auto_sync: false,
            sync_forward: Strategy::Prompt,
            sync_backward: Strategy::Never,
        }
    }
}

/// The position this device has for the document.
#[derive(Debug, Clone, PartialEq)]
pub struct LocalState {
    /// Current position as text, the same form as [`Progress::progress`].
    pub progress: String,
    /// Current fraction read, from 0.0 to 1.0.
    pub percentage: f64,
    /// Device name this device sends.
    pub device: String,
    /// Stable id of this installation.
    pub device_id: String,
    /// Unix seconds of the last page turn in this session, or 0 if there was
    /// none (`last_page_turn_timestamp`).
    pub last_page_turn: i64,
}

/// Decides what to do with `remote` when the sync runs without the user
/// asking (`KOSync:getProgress` with `interactive == false`).
///
/// The caller gets `None` from [`SyncClient::pull`](crate::SyncClient::pull)
/// when the server has no position. The plugin does nothing then, so the
/// caller should do nothing too.
///
/// A `Never` strategy leaves the local position alone. Close, suspend, and
/// periodic updates schedule pushes separately.
pub fn decide(local: &LocalState, remote: &Progress, policy: &Policy) -> Decision {
    // The plugin only registers pull events when auto sync is on
    // (`registerEvents`).
    if !policy.auto_sync {
        return Decision::Keep;
    }
    if let Some(done) = already_in_step(local, remote) {
        return done;
    }
    if remote_is_newer(local, remote) {
        // main.lua:904-917
        strategy_decision(policy.sync_forward)
    } else {
        // main.lua:918-931
        strategy_decision(policy.sync_backward)
    }
}

/// Decides what to do with `remote` after the user asks to pull
/// ("Pull progress from other devices now"). The plugin then moves the
/// reader without a prompt, whatever the strategies say (`main.lua:889-895`),
/// and ignores `auto_sync`.
pub fn decide_manual(local: &LocalState, remote: &Progress) -> Decision {
    already_in_step(local, remote).unwrap_or(Decision::PullRemote)
}

/// Checks that come before any direction logic. Both return `Keep`.
fn already_in_step(local: &LocalState, remote: &Progress) -> Option<Decision> {
    // main.lua:861-870. The plugin compares the device name with the model
    // name. This port compares the name this device sends.
    if remote.device == local.device && remote.device_id == local.device_id {
        return Some(Decision::Keep);
    }
    // main.lua:877-886. Equal percentage, or equal position text.
    if round_percent(local.percentage) == round_percent(remote.percentage)
        || local.progress == remote.progress
    {
        return Some(Decision::Keep);
    }
    None
}

/// Whether the remote position is newer than the last local page turn.
fn remote_is_newer(local: &LocalState, remote: &Progress) -> bool {
    match remote.timestamp {
        // main.lua:897-899. The server timestamp against the last page turn.
        Some(timestamp) => timestamp > local.last_page_turn,
        // main.lua:901-902. Older servers send no timestamp, so the
        // percentage decides.
        None => round_percent(remote.percentage) > round_percent(local.percentage),
    }
}

fn strategy_decision(strategy: Strategy) -> Decision {
    match strategy {
        Strategy::Silent => Decision::PullRemote,
        Strategy::Prompt => Decision::AskUser,
        Strategy::Never => Decision::Keep,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const THIS_DEVICE_ID: &str = "dev-this";

    fn local(percentage: f64, last_page_turn: i64) -> LocalState {
        LocalState {
            progress: "/body/p[10]".to_owned(),
            percentage,
            device: "Kobo Libra Colour".to_owned(),
            device_id: THIS_DEVICE_ID.to_owned(),
            last_page_turn,
        }
    }

    fn remote(percentage: f64, timestamp: Option<i64>) -> Progress {
        Progress {
            document: "doc".to_owned(),
            progress: "/body/p[99]".to_owned(),
            percentage,
            device: "Desktop".to_owned(),
            device_id: "dev-other".to_owned(),
            timestamp,
            metadata: None,
        }
    }

    fn policy(forward: Strategy, backward: Strategy) -> Policy {
        Policy {
            auto_sync: true,
            sync_forward: forward,
            sync_backward: backward,
        }
    }

    #[test]
    fn defaults_match_the_plugin() {
        let p = Policy::default();
        assert!(!p.auto_sync);
        assert_eq!(p.sync_forward, Strategy::Prompt);
        assert_eq!(p.sync_backward, Strategy::Never);
    }

    // main.lua:1062-1081 and registerEvents: no automatic pull without auto sync.
    #[test]
    fn auto_sync_off_keeps_everything() {
        let p = Policy {
            auto_sync: false,
            ..policy(Strategy::Silent, Strategy::Silent)
        };
        assert_eq!(
            decide(&local(0.2, 100), &remote(0.8, Some(900)), &p),
            Decision::Keep
        );
    }

    // main.lua:861-870. Same device name and device id: the plugin says the
    // progress comes from this device.
    #[test]
    fn same_device_keeps() {
        let mut r = remote(0.8, Some(900));
        r.device = "Kobo Libra Colour".to_owned();
        r.device_id = THIS_DEVICE_ID.to_owned();
        let p = policy(Strategy::Silent, Strategy::Silent);
        assert_eq!(decide(&local(0.2, 100), &r, &p), Decision::Keep);
    }

    // Both fields must match, as in `body.device == Device.model and body.device_id == self.device_id`.
    #[test]
    fn same_name_other_id_is_not_this_device() {
        let mut r = remote(0.8, Some(900));
        r.device = "Kobo Libra Colour".to_owned();
        r.device_id = "dev-other".to_owned();
        let p = policy(Strategy::Silent, Strategy::Silent);
        assert_eq!(decide(&local(0.2, 100), &r, &p), Decision::PullRemote);
    }

    // main.lua:877-886, percentage branch.
    #[test]
    fn equal_percentage_keeps() {
        let p = policy(Strategy::Silent, Strategy::Silent);
        assert_eq!(
            decide(&local(0.5, 100), &remote(0.5, Some(900)), &p),
            Decision::Keep
        );
    }

    // The percentage comparison uses the rounded values from `Math.roundPercent`.
    #[test]
    fn percentages_equal_after_rounding_keep() {
        let p = policy(Strategy::Silent, Strategy::Silent);
        assert_eq!(
            decide(&local(0.12349, 100), &remote(0.1234, Some(900)), &p),
            Decision::Keep
        );
    }

    // main.lua:877-878, the `body.progress == progress` branch.
    #[test]
    fn equal_position_text_keeps() {
        let mut r = remote(0.8, Some(900));
        r.progress = "/body/p[10]".to_owned();
        let p = policy(Strategy::Silent, Strategy::Silent);
        assert_eq!(decide(&local(0.2, 100), &r, &p), Decision::Keep);
    }

    // main.lua:897-899 and 904-907: remote newer, forward silent.
    #[test]
    fn newer_remote_silent_forward_pulls() {
        let p = policy(Strategy::Silent, Strategy::Never);
        assert_eq!(
            decide(&local(0.2, 100), &remote(0.8, Some(101)), &p),
            Decision::PullRemote
        );
    }

    // main.lua:908-916: remote newer, forward prompt.
    #[test]
    fn newer_remote_prompt_forward_asks() {
        let p = policy(Strategy::Prompt, Strategy::Never);
        assert_eq!(
            decide(&local(0.2, 100), &remote(0.8, Some(101)), &p),
            Decision::AskUser
        );
    }

    // The "Never" case for forward. The plugin keeps the local position,
    // and its close handler pushes it.
    #[test]
    fn newer_remote_never_forward_pushes_local() {
        let p = policy(Strategy::Never, Strategy::Silent);
        assert_eq!(
            decide(&local(0.2, 100), &remote(0.8, Some(101)), &p),
            Decision::Keep
        );
    }

    // main.lua:918-921: remote older, backward silent.
    #[test]
    fn older_remote_silent_backward_pulls() {
        let p = policy(Strategy::Never, Strategy::Silent);
        assert_eq!(
            decide(&local(0.8, 100), &remote(0.2, Some(50)), &p),
            Decision::PullRemote
        );
    }

    // main.lua:922-930: remote older, backward prompt.
    #[test]
    fn older_remote_prompt_backward_asks() {
        let p = policy(Strategy::Never, Strategy::Prompt);
        assert_eq!(
            decide(&local(0.8, 100), &remote(0.2, Some(50)), &p),
            Decision::AskUser
        );
    }

    // The default backward strategy is Never, so an older remote is overwritten
    // by the local position on close.
    #[test]
    fn older_remote_never_backward_pushes_local() {
        let p = policy(Strategy::Prompt, Strategy::Never);
        assert_eq!(
            decide(&local(0.8, 100), &remote(0.2, Some(50)), &p),
            Decision::Keep
        );
    }

    // main.lua:899. The comparison is strict, so a timestamp equal to the last
    // page turn counts as older.
    #[test]
    fn timestamp_equal_to_last_page_turn_is_older() {
        let p = policy(Strategy::Silent, Strategy::Never);
        assert_eq!(
            decide(&local(0.2, 100), &remote(0.8, Some(100)), &p),
            Decision::Keep
        );
    }

    // main.lua:901-902: no timestamp, so a larger remote percentage means newer.
    #[test]
    fn no_timestamp_uses_percentage_newer() {
        let p = policy(Strategy::Silent, Strategy::Never);
        assert_eq!(
            decide(&local(0.2, 100), &remote(0.8, None), &p),
            Decision::PullRemote
        );
    }

    // main.lua:901-902: no timestamp, smaller remote percentage means older.
    #[test]
    fn no_timestamp_uses_percentage_older() {
        let p = policy(Strategy::Never, Strategy::Silent);
        assert_eq!(
            decide(&local(0.8, 100), &remote(0.2, None), &p),
            Decision::PullRemote
        );
        let p = policy(Strategy::Never, Strategy::Never);
        assert_eq!(decide(&local(0.8, 100), &remote(0.2, None), &p), Decision::Keep);
    }

    // main.lua:889-895. A manual pull moves the reader without a prompt.
    #[test]
    fn manual_pull_moves_without_asking() {
        assert_eq!(
            decide_manual(&local(0.2, 100), &remote(0.8, Some(101))),
            Decision::PullRemote
        );
        assert_eq!(
            decide_manual(&local(0.8, 100), &remote(0.2, Some(50))),
            Decision::PullRemote
        );
    }

    // main.lua:861-870 applies to manual pulls too.
    #[test]
    fn manual_pull_from_this_device_keeps() {
        let mut r = remote(0.8, Some(101));
        r.device = "Kobo Libra Colour".to_owned();
        r.device_id = THIS_DEVICE_ID.to_owned();
        assert_eq!(decide_manual(&local(0.2, 100), &r), Decision::Keep);
    }

    // main.lua:877-886 applies to manual pulls too.
    #[test]
    fn manual_pull_at_same_position_keeps() {
        assert_eq!(
            decide_manual(&local(0.5, 100), &remote(0.5, Some(101))),
            Decision::Keep
        );
    }
}
