//! Short socket waits let a worker cancel even when a server stops sending.
use std::io;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::time::{Duration, Instant};
use ureq::unversioned::resolver::DefaultResolver;
use ureq::unversioned::transport::{
    Buffers, ConnectionDetails, Connector, DefaultConnector, NextTimeout, Transport,
};

pub(crate) fn agent(config: ureq::config::Config, cancel: Arc<AtomicBool>) -> ureq::Agent {
    ureq::Agent::with_parts(
        config,
        DefaultConnector::default().chain(CancelConnector(cancel)),
        DefaultResolver::default(),
    )
}

#[derive(Debug)]
struct CancelConnector(Arc<AtomicBool>);
impl<T: Transport> Connector<T> for CancelConnector {
    type Out = CancelTransport<T>;
    fn connect(&self, _: &ConnectionDetails, transport: Option<T>) -> Result<Option<Self::Out>, ureq::Error> {
        Ok(transport.map(|inner| CancelTransport {
            inner,
            cancel: self.0.clone(),
        }))
    }
}
#[derive(Debug)]
struct CancelTransport<T> {
    inner: T,
    cancel: Arc<AtomicBool>,
}
impl<T: Transport> CancelTransport<T> {
    fn check(&self) -> Result<(), ureq::Error> {
        if self.cancel.load(Ordering::Relaxed) {
            Err(ureq::Error::Io(io::Error::other("cancelled")))
        } else {
            Ok(())
        }
    }
}
impl<T: Transport> Transport for CancelTransport<T> {
    fn buffers(&mut self) -> &mut dyn Buffers {
        self.inner.buffers()
    }
    fn transmit_output(&mut self, amount: usize, timeout: NextTimeout) -> Result<(), ureq::Error> {
        self.check()?;
        self.inner.transmit_output(amount, timeout)
    }
    fn await_input(&mut self, timeout: NextTimeout) -> Result<bool, ureq::Error> {
        let start = Instant::now();
        loop {
            self.check()?;
            let left = timeout.after.saturating_sub(start.elapsed());
            if left.is_zero() {
                return Err(ureq::Error::Timeout(timeout.reason));
            }
            let short = NextTimeout {
                after: ureq::unversioned::transport::time::Duration::Exact(
                    left.min(Duration::from_millis(100)),
                ),
                reason: timeout.reason,
            };
            match self.inner.await_input(short) {
                Err(ureq::Error::Timeout(_)) => continue,
                Err(ureq::Error::Io(e))
                    if matches!(e.kind(), io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock) =>
                {
                    continue
                }
                result => {
                    self.check()?;
                    return result;
                }
            }
        }
    }
    fn is_open(&mut self) -> bool {
        !self.cancel.load(Ordering::Relaxed) && self.inner.is_open()
    }
    fn is_tls(&self) -> bool {
        self.inner.is_tls()
    }
}
