//! Script plans run in order on one worker. The app polls completion.
use kaia_platform::plan::{execute, Outcome, Plan};
use kaia_platform::sysfs::{Root, SystemIo};
use std::sync::mpsc::{self, Receiver, Sender};

pub(super) struct Scripts {
    send: Sender<(u64, Plan)>,
    replies: Receiver<(u64, Outcome)>,
    pending: usize,
}
impl Default for Scripts {
    fn default() -> Self {
        let (send, requests) = mpsc::channel::<(u64, Plan)>();
        let (done, replies) = mpsc::channel();
        std::thread::spawn(move || {
            let mut io = SystemIo::new(Root::new("/"));
            while let Ok((generation, plan)) = requests.recv() {
                let result = execute(&plan, &mut io, &mut std::thread::sleep);
                if done.send((generation, result)).is_err() {
                    break;
                }
            }
        });
        Self {
            send,
            replies,
            pending: 0,
        }
    }
}
impl Scripts {
    pub fn submit(&mut self, generation: u64, plan: Plan) {
        if self.send.send((generation, plan)).is_ok() {
            self.pending += 1;
        }
    }
    pub fn poll(&mut self) -> Option<(u64, Outcome)> {
        let result = self.replies.try_recv().ok()?;
        self.pending -= 1;
        Some(result)
    }
    pub fn busy(&self) -> bool {
        self.pending > 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};
    #[test]
    fn slow_script_submission_and_poll_do_not_wait() {
        let mut scripts = Scripts::default();
        let start = Instant::now();
        scripts.submit(
            7,
            Plan::new(vec![kaia_platform::plan::Step::sleep(
                Duration::from_millis(200),
                "test",
            )]),
        );
        assert!(scripts.poll().is_none());
        assert!(start.elapsed() < Duration::from_millis(100));
        assert!(scripts.busy());
        let deadline = start + Duration::from_secs(2);
        loop {
            if let Some((generation, outcome)) = scripts.poll() {
                assert_eq!(generation, 7);
                assert_eq!(outcome, Outcome::Completed);
                break;
            }
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(!scripts.busy());
    }
}
