//! The scheduler runs suspend timers and frontlight ramp steps on the app thread.

use std::cell::RefCell;
use std::collections::VecDeque;
use std::rc::Rc;
use std::time::Duration;

use kaia_events::{Ctx, Event, Refresh, RefreshMode, ShowOpts, TaskId, WindowId};
use kaia_platform::frontlight::RampStep;
use kaia_platform::power::{PowerContext, ResumeContext};
use kaia_platform::suspend::{Effect, PrepareResult, SuspendFlow, Timer, WakeupCause, SUSPEND_WAIT_TIMEOUT};
use kaia_ui::Window;

use super::App;
use crate::backend::Backend;
use crate::surfaces::{settings, Route};

#[derive(Default)]
pub(super) struct State {
    flow: SuspendFlow,
    timers: Vec<(Timer, TaskId)>,
    queue: Rc<RefCell<VecDeque<Due>>>,
    screen: Option<WindowId>,
    restore_light: bool,
    ramp_generation: u64,
}

enum Due {
    Timer(Timer),
    Light(u64, RampStep),
    Resume(kaia_platform::plan::Plan),
}

impl<B: Backend> App<B> {
    pub fn sleeping(&self) -> bool {
        self.power.screen.is_some()
    }

    pub(super) fn sleep(&mut self) {
        if self.sleeping() {
            return;
        }
        self.broadcast_app(Event::new("Suspend"));
        if let Err(e) = self.services.settings.flush(&self.services.settings_path()) {
            self.notices.push(format!("Cannot save settings: {e}"));
        }
        let book = self.router.routes().iter().find_map(|r| match r {
            Route::Reader { book } => Some(*book),
            _ => None,
        });
        let window = Window::new(
            settings::sleep::sleep_screen(&self.services, self.screen, book),
            self.services.theme.clone(),
            self.services.assets.clone(),
            self.screen,
        );
        let id = window.show(
            &mut self.ctx,
            ShowOpts {
                modal: true,
                covers_fullscreen: true,
                refresh: Some(Refresh::new(RefreshMode::Full)),
                ..ShowOpts::default()
            },
        );
        self.transient.push(window);
        self.power.screen = Some(id);
        self.power.restore_light = self.services.platform.frontlight.is_on();
        if self.power.restore_light {
            self.toggle_light();
        }
        let effects = self.power.flow.reschedule_suspend(SUSPEND_WAIT_TIMEOUT);
        self.power_effects(effects);
    }

    pub(super) fn resume(&mut self) {
        let effects = self.power.flow.resume();
        self.power_effects(effects);
        let Some(id) = self.power.screen.take() else {
            return;
        };
        self.stack
            .close(id, Some(Refresh::new(RefreshMode::Full)), &mut self.ctx);
        let mut plan = self.services.platform.power.resume(ResumeContext {
            sync_system_clock: true,
            ..ResumeContext::default()
        });
        // Split the plan's short delay as well, so input remains responsive on resume.
        if let Some(index) = plan
            .steps
            .iter()
            .position(|s| matches!(s.action, kaia_platform::plan::Action::Sleep(_)))
        {
            let rest = plan.steps.split_off(index + 1);
            let delay = match plan.steps.pop().unwrap().action {
                kaia_platform::plan::Action::Sleep(d) => d,
                _ => unreachable!(),
            };
            self.services.platform.run_plan(&plan);
            let queue = self.power.queue.clone();
            self.ctx.scheduler.schedule_in(
                delay.as_secs_f64(),
                Box::new(move |_: &mut Ctx| {
                    queue
                        .borrow_mut()
                        .push_back(Due::Resume(kaia_platform::plan::Plan::new(rest)));
                }),
            );
        } else {
            self.services.platform.run_plan(&plan);
        }
        if self.power.restore_light {
            self.toggle_light();
        }
        self.broadcast_app(Event::new("Resume"));
    }

    pub(super) fn toggle_light(&mut self) {
        self.power.ramp_generation += 1;
        let generation = self.power.ramp_generation;
        let Some(ramp) = self.services.platform.frontlight.begin_toggle() else {
            return;
        };
        let mut delay = Duration::ZERO;
        for step in ramp.steps {
            delay += step.delay;
            let queue = self.power.queue.clone();
            self.ctx.scheduler.schedule_in(
                delay.as_secs_f64(),
                Box::new(move |_: &mut Ctx| {
                    queue.borrow_mut().push_back(Due::Light(generation, step));
                }),
            );
        }
    }

    pub(super) fn power_due(&mut self) {
        loop {
            let next = self.power.queue.borrow_mut().pop_front();
            let Some(next) = next else {
                break;
            };
            match next {
                Due::Timer(timer) => {
                    if !self.power.timers.iter().any(|(t, _)| *t == timer) {
                        continue;
                    }
                    self.power.timers.retain(|(t, _)| *t != timer);
                    let effects = match timer {
                        Timer::Suspend => self.power.flow.suspend_timer_fired(),
                        Timer::FinishSuspend => self.power.flow.finish_timer_fired(),
                        Timer::UnexpectedWakeupCheck => {
                            self.power.flow.check_timer_fired(WakeupCause::Unexpected)
                        }
                    };
                    self.power_effects(effects);
                }
                Due::Light(generation, step) if generation == self.power.ramp_generation => {
                    if let Err(e) = self.services.platform.frontlight.apply_ramp_step(&step) {
                        self.services.platform.frontlight.cancel_ramp();
                        self.power.ramp_generation += 1;
                        self.notices.push(format!("Cannot change the light: {e}"));
                    }
                }
                Due::Resume(plan) => {
                    self.services.platform.run_plan(&plan);
                }
                _ => {}
            }
        }
    }

    fn power_effects(&mut self, effects: Vec<Effect>) {
        for effect in effects {
            match effect {
                Effect::Cancel(timer) => {
                    for (_, task) in self.power.timers.iter().filter(|(t, _)| *t == timer) {
                        self.ctx.scheduler.unschedule(*task);
                    }
                    self.power.timers.retain(|(t, _)| *t != timer);
                }
                Effect::Schedule { timer, delay } => {
                    let queue = self.power.queue.clone();
                    let task = self.ctx.scheduler.schedule_in(
                        delay.as_secs_f64(),
                        Box::new(move |_: &mut Ctx| {
                            queue.borrow_mut().push_back(Due::Timer(timer));
                        }),
                    );
                    self.power.timers.push((timer, task));
                }
                Effect::PrepareSuspend => {
                    let platform = &mut self.services.platform;
                    let plan = platform.power.prepare_suspend(PowerContext {
                        wifi_on: platform.network.is_wifi_on(),
                        charging: platform.battery.is_charging().unwrap_or(false),
                    });
                    let result = platform.run_plan(&plan);
                    let next = self.power.flow.prepare_done(PrepareResult::from_outcome(&result));
                    self.power_effects(next);
                }
                Effect::RunFinishSuspend => {
                    let plan = self.services.platform.power.finish_suspend();
                    self.services.platform.run_plan(&plan);
                    let next = self.power.flow.finish_done();
                    self.power_effects(next);
                }
                Effect::GiveUp => self.broadcast_app(Event::new("UnexpectedWakeupLimit")),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{AppOptions, HeadlessBackend, ScriptInput, Services};
    use kaia_events::ManualClock;
    use kaia_platform::{Env, Platform};

    #[test]
    fn power_press_cancels_the_two_second_finish_timer() {
        let dir = std::env::temp_dir().join(format!("kaia-power-timer-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let platform = Platform::detect_in(
            &dir,
            &Env {
                home: Some(dir.clone()),
                ..Env::default()
            },
        )
        .unwrap();
        let panel = kaia_display::PanelSpec::LIBRA_COLOUR;
        let services = Services::open(platform, panel, Some(dir.join("data"))).unwrap();
        let clock = ManualClock::new();
        let mut app = App::new(
            HeadlessBackend::new(panel, ScriptInput::idle()),
            services,
            Box::new(clock.clone()),
            AppOptions::default(),
        );
        app.start().unwrap();
        app.sleep();
        let cancel = app.power.flow.resume();
        app.power_effects(cancel);
        let effects = app.power.flow.prepare_done(PrepareResult::Done);
        app.power_effects(effects);
        assert!(app.power.flow.finish_pending());
        clock.advance(Duration::from_secs(1));
        app.key(kaia_input::Key::Power).unwrap();
        assert!(!app.power.flow.finish_pending());
        clock.advance(Duration::from_secs(2));
        app.process(Vec::new()).unwrap();
        assert!(!app.sleeping());
        assert!(app.power.timers.is_empty());
    }
}
