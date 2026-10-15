//! Wi-Fi requests run off the UI thread and return ownership of the radio.
use kaia_platform::{
    network::Network,
    wpa::{SavedNetwork, ScanResult},
    PlatformError,
};
use std::sync::mpsc::{self, Receiver};
pub(super) enum Request {
    Scan,
    Saved,
    Join(String, Option<String>),
    Forget(u32),
    Toggle,
}
pub(super) enum Reply {
    Scan(Vec<ScanResult>),
    Saved(Vec<SavedNetwork>),
    Joined,
    Forgotten,
    Toggled { requested_on: bool },
}
pub(super) type Completion = (Box<dyn Network>, Result<Reply, PlatformError>);

/// A status-only stand-in while the worker owns the radio. It refuses concurrent operations.
struct BusyRadio {
    on: bool,
    connected: bool,
}
impl Network for BusyRadio {
    fn is_wifi_on(&self) -> bool {
        self.on
    }
    fn is_connected(&self) -> bool {
        self.connected
    }
    fn enable_wifi(&mut self) -> Result<(), PlatformError> {
        Err(PlatformError::NotSupported("Wi-Fi busy"))
    }
    fn disable_wifi(&mut self) -> Result<(), PlatformError> {
        self.enable_wifi()
    }
    fn obtain_ip(&mut self) -> Result<(), PlatformError> {
        self.enable_wifi()
    }
    fn release_ip(&mut self) -> Result<(), PlatformError> {
        self.enable_wifi()
    }
    fn scan(&mut self) -> Result<Vec<ScanResult>, PlatformError> {
        Err(PlatformError::NotSupported("Wi-Fi busy"))
    }
    fn connect(&mut self, _: &str, _: Option<&str>) -> Result<(), PlatformError> {
        self.enable_wifi()
    }
}

pub(super) fn start(radio: &mut Box<dyn Network>, request: Request) -> Receiver<Completion> {
    let placeholder = BusyRadio {
        on: radio.is_wifi_on(),
        connected: radio.is_connected(),
    };
    let mut radio = std::mem::replace(radio, Box::new(placeholder));
    let (send, receive) = mpsc::channel();
    std::thread::spawn(move || {
        let result = match request {
            Request::Scan => radio.scan().map(Reply::Scan),
            Request::Saved => radio.saved_networks().map(Reply::Saved),
            Request::Join(name, password) => radio
                .connect(&name, password.as_deref())
                .and_then(|()| radio.obtain_ip())
                .map(|()| Reply::Joined),
            Request::Forget(id) => radio.forget_network(id).map(|()| Reply::Forgotten),
            Request::Toggle => {
                let requested_on = !radio.is_wifi_on();
                if requested_on {
                    radio.enable_wifi()
                } else {
                    radio.disable_wifi()
                }
                .map(|()| Reply::Toggled { requested_on })
            }
        };
        let _ = send.send((radio, result));
    });
    receive
}
