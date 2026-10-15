//! Network time. With `device.network_time` on, airplane mode off and Wi-Fi
//! connected, Kaia sets the clock from pool.ntp.org at most once every six
//! hours. The switch is off by default, and while it is off no packet is sent.
//!
//! On a Kobo the sync is the BusyBox `ntpd` and `hwclock` steps from
//! `kaia_platform::ntp::device_steps`. They run on the script worker, in order
//! with the Wi-Fi scripts. On the desktop the app asks the server on a short
//! thread and reports the offset. The desktop clock does not change.

use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::time::SystemTime;

use kaia_i18n::{format as tr_format, tr};
use kaia_platform::ntp::{self, NtpError, Sample};
use kaia_platform::plan::Outcome;
use kaia_platform::Kind;

use super::App;
use crate::backend::Backend;
use crate::surfaces::settings::keys;

/// The script worker job of a device sync. Like the other device jobs, it counts down from `u64::MAX`.
pub(super) const CLOCK_JOB: u64 = u64::MAX - 3;

pub(super) struct State {
    /// When the last sync started. None until the first one starts.
    last_attempt: Option<SystemTime>,
    /// A device sync is on the script worker.
    device_running: bool,
    /// A desktop query in flight. Its result arrives here.
    query: Option<Receiver<Result<Sample, NtpError>>>,
    /// The server a desktop query asks. Tests point it at a local responder.
    server: String,
}

impl State {
    pub(super) fn new() -> Self {
        Self {
            last_attempt: None,
            device_running: false,
            query: None,
            server: ntp::POOL_ADDRESS.to_string(),
        }
    }
}

impl<B: Backend> App<B> {
    /// Reads a finished desktop query and starts a sync when one is due. Runs on each loop pass.
    pub(super) fn clock_due(&mut self) {
        self.clock_poll();
        if !self.clock_allowed() {
            return;
        }
        let connected = self.services.platform.network.is_connected();
        self.clock_start(connected);
    }

    /// The switch is on and airplane mode is off.
    fn clock_allowed(&self) -> bool {
        keys::network_time(&self.services.settings) && !keys::airplane_mode(&self.services.settings)
    }

    /// Starts one sync when the switch allows it, Wi-Fi is connected and six hours have passed.
    fn clock_start(&mut self, connected: bool) {
        if !self.clock_allowed() || self.clock.device_running || self.clock.query.is_some() {
            return;
        }
        let now = SystemTime::now();
        if !ntp::due(connected, self.clock.last_attempt, now) {
            return;
        }
        self.clock.last_attempt = Some(now);
        match self.services.platform.kind {
            Kind::Kobo(_) => {
                self.clock.device_running = true;
                self.power.scripts.submit(CLOCK_JOB, ntp::device_steps());
            }
            Kind::Desktop => {
                let (send, receive) = mpsc::channel();
                let server = self.clock.server.clone();
                std::thread::spawn(move || {
                    let _ = send.send(ntp::query(&server, ntp::TIMEOUT));
                });
                self.clock.query = Some(receive);
            }
        }
    }

    /// Turns a finished desktop query into a notice. The offset is server time minus local time.
    fn clock_poll(&mut self) {
        let Some(receive) = &self.clock.query else {
            return;
        };
        let result = match receive.try_recv() {
            Ok(result) => result,
            Err(TryRecvError::Empty) => return,
            Err(TryRecvError::Disconnected) => Err(NtpError::Timeout),
        };
        self.clock.query = None;
        let note = match result {
            Ok(sample) => tr_format(
                &tr("Network time: pool.ntp.org gives an offset of %1 seconds. Desktop builds do not change the clock."),
                &[&format!("{:+.1}", sample.offset_seconds)],
            ),
            Err(_) => tr("Network time: could not get the time from pool.ntp.org."),
        };
        self.notices.push(note);
    }

    /// Acts on a device sync when the script worker returns it. `ntpd` failing
    /// skips the RTC copy, so the clocks stay as they were.
    pub(super) fn clock_done(&mut self, outcome: Outcome) {
        self.clock.device_running = false;
        let note = if matches!(outcome, Outcome::Completed) {
            tr("Network time: the clock is set from pool.ntp.org.")
        } else {
            tr("Network time: could not set the clock from pool.ntp.org. The clock is unchanged.")
        };
        self.notices.push(note);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{AppOptions, HeadlessBackend, ScriptInput, Services};
    use kaia_events::{ManualClock, Value};
    use kaia_platform::{Env, Platform};
    use std::net::UdpSocket;
    use std::thread;
    use std::time::{Duration, Instant, UNIX_EPOCH};

    fn app() -> (tempfile::TempDir, App<HeadlessBackend>) {
        let dir = tempfile::tempdir().unwrap();
        let platform = Platform::detect_in(
            dir.path(),
            &Env {
                home: Some(dir.path().into()),
                ..Env::default()
            },
        )
        .unwrap();
        let panel = kaia_display::PanelSpec::LIBRA_COLOUR;
        let services = Services::open(platform, panel, Some(dir.path().join("data"))).unwrap();
        let mut app = App::new(
            HeadlessBackend::new(panel, ScriptInput::idle()),
            services,
            Box::new(ManualClock::new()),
            AppOptions::default(),
        );
        app.start().unwrap();
        (dir, app)
    }

    /// A socket that never replies. [`nothing_received`] checks whether a packet reached it.
    fn silent_server() -> (UdpSocket, String) {
        let socket = UdpSocket::bind("127.0.0.1:0").unwrap();
        socket.set_read_timeout(Some(Duration::from_millis(300))).unwrap();
        let address = socket.local_addr().unwrap().to_string();
        (socket, address)
    }

    fn nothing_received(socket: &UdpSocket) -> bool {
        let mut buffer = [0u8; 512];
        socket.recv_from(&mut buffer).is_err()
    }

    /// A server whose clock is 3 seconds ahead. It answers one request with the first byte `first`.
    fn answering_server(first: u8) -> String {
        let socket = UdpSocket::bind("127.0.0.1:0").unwrap();
        socket.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
        let address = socket.local_addr().unwrap().to_string();
        thread::spawn(move || {
            let mut buffer = [0u8; 512];
            let Ok((_, peer)) = socket.recv_from(&mut buffer) else {
                return;
            };
            let since = (SystemTime::now() + Duration::from_secs(3))
                .duration_since(UNIX_EPOCH)
                .unwrap();
            let seconds = since.as_secs() + 2_208_988_800;
            let fraction = (u64::from(since.subsec_nanos()) << 32) / 1_000_000_000;
            let stamp = ((seconds << 32) | fraction).to_be_bytes();
            let mut reply = vec![0u8; 48];
            reply[0] = first;
            reply[1] = 2;
            reply[24..32].copy_from_slice(&buffer[40..48]);
            reply[32..40].copy_from_slice(&stamp);
            reply[40..48].copy_from_slice(&stamp);
            let _ = socket.send_to(&reply, peer);
        });
        address
    }

    /// Waits for the next notice of the app and returns it.
    fn next_notice(app: &mut App<HeadlessBackend>, before: usize) -> String {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            app.clock_poll();
            if app.notices().len() > before {
                return app.notices().last().unwrap().clone();
            }
            assert!(Instant::now() < deadline, "no notice arrived");
            thread::sleep(Duration::from_millis(10));
        }
    }

    fn switch_on(app: &mut App<HeadlessBackend>) {
        app.services
            .settings
            .save(keys::DEVICE_NETWORK_TIME, Value::from(true));
    }

    #[test]
    fn off_by_default_sends_no_packet() {
        let (_dir, mut app) = app();
        let (socket, address) = silent_server();
        app.clock.server = address;
        assert!(!keys::network_time(&app.services.settings), "off by default");
        app.clock_due();
        app.clock_start(true);
        assert!(app.clock.query.is_none());
        assert!(nothing_received(&socket));
    }

    #[test]
    fn airplane_mode_sends_no_packet() {
        let (_dir, mut app) = app();
        let (socket, address) = silent_server();
        app.clock.server = address;
        switch_on(&mut app);
        app.services
            .settings
            .save(keys::NETWORK_AIRPLANE_MODE, Value::from(true));
        app.clock_start(true);
        assert!(app.clock.query.is_none());
        assert!(nothing_received(&socket));
    }

    #[test]
    fn no_wifi_sends_no_packet() {
        let (_dir, mut app) = app();
        let (socket, address) = silent_server();
        app.clock.server = address;
        switch_on(&mut app);
        app.clock_start(false);
        assert!(app.clock.query.is_none());
        assert!(nothing_received(&socket));
    }

    #[test]
    fn desktop_reports_the_offset_and_does_not_retry_within_six_hours() {
        let (_dir, mut app) = app();
        app.clock.server = answering_server(0x24);
        switch_on(&mut app);
        let before = app.notices().len();
        app.clock_start(true);
        let note = next_notice(&mut app, before);
        assert!(note.contains("+3.0"), "{note}");
        assert!(note.contains("do not change the clock"), "{note}");

        // A second check inside six hours sends nothing.
        let (socket, address) = silent_server();
        app.clock.server = address;
        app.clock_start(true);
        assert!(app.clock.query.is_none());
        assert!(nothing_received(&socket));
    }

    #[test]
    fn a_refused_reply_is_reported_and_counts_as_the_attempt() {
        let (_dir, mut app) = app();
        app.clock.server = answering_server(0x23); // client mode, refused
        switch_on(&mut app);
        let before = app.notices().len();
        app.clock_start(true);
        let note = next_notice(&mut app, before);
        assert!(note.contains("could not get the time"), "{note}");

        let (socket, address) = silent_server();
        app.clock.server = address;
        app.clock_start(true);
        assert!(app.clock.query.is_none(), "a failed attempt also waits six hours");
        assert!(nothing_received(&socket));
    }
}
