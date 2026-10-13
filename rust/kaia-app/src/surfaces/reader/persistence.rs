//! Position changes share one save deadline and one sync update path.
use super::state::Core;
use crate::surface::SurfaceEnv;
use kaia_book::BookSession;
use kaia_events::TaskId;
use std::time::Duration;

#[derive(Default)]
pub(super) struct Persistence {
    position: Option<String>,
    document: Option<String>,
    due: Option<Duration>,
    wake: Option<TaskId>,
    stats: Option<kaia_stats::Stats>,
    stats_started: bool,
    epoch: Option<i64>,
    paused: bool,
    save_pending: bool,
}
impl Persistence {
    fn now(&mut self, env: &SurfaceEnv<'_>) -> i64 {
        let elapsed = env.ctx.scheduler.now().as_secs() as i64;
        *self
            .epoch
            .get_or_insert_with(|| crate::sync::now_unix() - elapsed)
            + elapsed
    }
    pub fn tick(&mut self, c: &mut Core, env: &mut SurfaceEnv<'_>) {
        let Some(session) = c.session.as_mut() else {
            if let Some(document) = &self.document {
                env.services.sync.defer_pending(document, env.ctx.scheduler.now());
            }
            return;
        };
        self.document = Some(session.identity().partial_md5.clone());
        let status_key = format!("book.status.{}", session.identity().partial_md5);
        if let Some(status) = env
            .services
            .settings
            .read(status_key.clone())
            .as_str()
            .map(str::to_owned)
        {
            match session.set_status(Some(&status)) {
                Ok(()) => {
                    env.services.settings.del(status_key);
                    if let Err(e) = env.services.settings.flush(&env.services.settings_path()) {
                        env.notice(format!("Cannot save settings: {e}"));
                    }
                }
                Err(e) => env.notice(format!("Cannot save book status: {e}")),
            }
        }
        let now = self.now(env);
        if !self.stats_started {
            self.stats_started = true;
            let row = env.services.library.book(c.book).ok().flatten();
            let result = kaia_stats::Stats::open(env.services.paths.data_dir.join("statistics.sqlite3"))
                .and_then(|mut stats| {
                    stats.start_book(
                        &session.identity().partial_md5,
                        &c.title,
                        row.as_ref().and_then(|b| b.authors.as_deref()).unwrap_or("N/A"),
                        session.page_count() as i64,
                        session.current().page as i64 + 1,
                        now,
                    )?;
                    if self.paused {
                        stats.on_suspend(now)?;
                    }
                    Ok(stats)
                });
            match result {
                Ok(stats) => self.stats = Some(stats),
                Err(e) => env.notice(format!("Cannot record reading time: {e}")),
            }
        }
        let position = crate::sync::position(session);
        if self.position.as_ref() != Some(&position) {
            c.last_page_turn = crate::sync::now_unix();
            self.position = Some(position.clone());
            // Save within five seconds even during continuous page turns.
            if self.due.is_none() {
                self.schedule(env);
            }
            if let Some(stats) = &mut self.stats {
                let result = stats
                    .pages_changed(session.page_count() as i64)
                    .and_then(|()| stats.page_turn(session.current().page as i64 + 1, now));
                if let Err(e) = result {
                    env.notice(format!("Cannot record reading time: {e}"));
                }
            }
            if let Some(due) = c
                .periodic_sync
                .page_update(&position, env.ctx.scheduler.now(), Some(10))
            {
                env.services.sync.periodic(
                    &env.services.settings,
                    session,
                    env.services.paths.data_dir.join("sync-queue.json"),
                    due,
                );
            }
        }
        if self.save_pending || self.due.is_some_and(|due| due <= env.ctx.scheduler.now()) {
            self.due = None;
            if !self.save(c, env) {
                self.schedule(env);
            }
        }
    }
    fn schedule(&mut self, env: &mut SurfaceEnv<'_>) {
        let due = env.ctx.scheduler.now() + Duration::from_secs(5);
        self.due = Some(due);
        if let Some(wake) = self.wake.take() {
            env.ctx.scheduler.unschedule(wake);
        }
        self.wake = Some(env.ctx.scheduler.schedule_at(due, Box::new(|_| {})));
    }
    pub fn save(&mut self, c: &mut Core, env: &mut SurfaceEnv<'_>) -> bool {
        let Some(session) = c.session.as_mut() else {
            self.save_pending = c.job.is_some();
            return !self.save_pending;
        };
        if let Err(e) = session.save() {
            env.notice(format!(
                "Cannot save this book. Free storage, then try again. {e}"
            ));
            return false;
        }
        self.save_pending = false;
        if let Err(e) = update_index(session, c.book, env) {
            env.notice(e);
        }
        true
    }
    pub fn event(&mut self, event: &str, env: &mut SurfaceEnv<'_>) {
        if event == "Suspend" {
            self.paused = true;
        }
        if event == "Resume" {
            self.paused = false;
        }
        let now = self.now(env);
        if let Some(stats) = &mut self.stats {
            let result = match event {
                "Suspend" => stats.on_suspend(now),
                "Resume" => stats.on_resume(now),
                "Close" => stats.end_book(now),
                _ => stats.flush(),
            };
            if let Err(e) = result {
                env.notice(format!("Cannot save reading time: {e}"));
            }
        }
        if event == "Close" {
            if let Some(wake) = self.wake.take() {
                env.ctx.scheduler.unschedule(wake);
            }
        }
    }
}
pub(super) fn update_index(session: &BookSession, book: i64, env: &mut SurfaceEnv<'_>) -> Result<(), String> {
    env.services
        .library
        .record_open(book, session.current().percent)
        .map_err(|e| e.to_string())?;
    let status = match session.status() {
        Some("complete") => kaia_library::Status::Complete,
        Some("abandoned") => kaia_library::Status::Abandoned,
        _ => kaia_library::Status::Reading,
    };
    env.services
        .library
        .set_status(book, status)
        .map_err(|e| e.to_string())
}
