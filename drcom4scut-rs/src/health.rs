//! Allow internal retries, but bound prolonged failure without healthy evidence.
//! Timers use monotonic time; sleeping and unreadable logs grant a fresh grace.
use crate::{
    logparse::{classify, Signal},
    model::LinkState,
};
use std::time::{Duration, Instant};
const STALE: Duration = Duration::from_secs(180);
const UNHEALTHY_LIMIT: Duration = Duration::from_secs(600);
const STABLE: Duration = Duration::from_secs(120);
const POLL_GAP: Duration = Duration::from_secs(30);

pub struct HealthMonitor {
    last_activity: Instant,
    recovery_since: Instant,
    last_health: Option<Instant>,
    healthy_since: Option<Instant>,
    last_poll: Instant,
    observed_activity: bool,
    scheduled_wait: bool,
    state: LinkState,
}
pub struct HealthDecision {
    pub state: LinkState,
    pub restart_stalled: bool,
    pub stable: bool,
    pub monitoring_unavailable: bool,
}
impl HealthMonitor {
    pub fn new(now: Instant) -> Self {
        Self {
            last_activity: now,
            recovery_since: now,
            last_health: None,
            healthy_since: None,
            last_poll: now,
            observed_activity: false,
            scheduled_wait: false,
            state: LinkState::Connecting,
        }
    }
    pub fn observe(&mut self, now: Instant, lines: Option<&[String]>) -> HealthDecision {
        // Sleep/resume and a blocked UI timer are not evidence of a dead core.
        if now.duration_since(self.last_poll) > POLL_GAP {
            self.last_activity = now;
            self.recovery_since = now;
            self.last_health = None;
            self.healthy_since = None;
        }
        self.last_poll = now;
        if let Some(lines) = lines {
            for line in lines {
                if line.trim().is_empty() {
                    continue;
                }
                self.last_activity = now;
                self.observed_activity = true;
                match classify(line) {
                    Signal::Healthy => {
                        self.last_health = Some(now);
                        self.recovery_since = now;
                        self.healthy_since.get_or_insert(now);
                        self.scheduled_wait = false;
                        self.state = LinkState::Online;
                    }
                    Signal::Error => {
                        self.healthy_since = None;
                        self.state = LinkState::Degraded;
                    }
                    Signal::Waiting => {
                        self.healthy_since = None;
                        self.scheduled_wait = true;
                        self.state = LinkState::Waiting;
                        self.recovery_since = now;
                    }
                    Signal::Other
                        if line.contains("Will try reconnect in")
                            || line.contains("Will try restart in")
                            || line.contains("Can't get ethernet device") =>
                    {
                        self.healthy_since = None;
                        self.scheduled_wait = false;
                        self.state = LinkState::Degraded;
                    }
                    _ => {}
                }
            }
        } else {
            // Log access failure must never turn into a destructive restart.
            self.last_activity = now;
            self.recovery_since = now;
            self.healthy_since = None;
        }
        let fresh_health = self
            .last_health
            .is_some_and(|t| now.duration_since(t) <= STALE);
        if self.state == LinkState::Online && !fresh_health {
            self.state = LinkState::Degraded;
            self.healthy_since = None;
        }
        if self.scheduled_wait {
            self.recovery_since = now;
        }
        HealthDecision {
            state: self.state,
            restart_stalled: lines.is_some()
                && !self.scheduled_wait
                && ((self.observed_activity && now.duration_since(self.last_activity) > STALE)
                    || now.duration_since(self.recovery_since) > UNHEALTHY_LIMIT),
            monitoring_unavailable: lines.is_none(),
            stable: fresh_health
                && self.state == LinkState::Online
                && self
                    .last_health
                    .zip(self.healthy_since)
                    .is_some_and(|(latest, start)| latest.duration_since(start) >= STABLE),
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn observe(m: &mut HealthMonitor, t: Instant, s: &str) -> HealthDecision {
        m.observe(t, Some(&s.lines().map(str::to_owned).collect::<Vec<_>>()))
    }
    #[test]
    fn repeated_errors_and_empty_startup_have_bounded_recovery() {
        let t = Instant::now();
        for repeated_error in [false, true] {
            let mut m = HealthMonitor::new(t);
            for seconds in (0..=602).step_by(2) {
                let line = if repeated_error && seconds % 14 == 0 {
                    "Can't get ethernet device, try again in 15 second(s)"
                } else {
                    ""
                };
                let d = observe(&mut m, t + Duration::from_secs(seconds), line);
                assert_eq!(d.restart_stalled, seconds > 600);
            }
        }
    }
    #[test]
    fn unreadable_logs_diagnosed_without_restart_and_return_gets_grace() {
        let t = Instant::now();
        let mut m = HealthMonitor::new(t);
        for seconds in (0..7200).step_by(2) {
            let d = m.observe(t + Duration::from_secs(seconds), None);
            assert!(d.monitoring_unavailable);
            assert!(!d.restart_stalled);
        }
        let d = observe(&mut m, t + Duration::from_secs(7200), "");
        assert!(!d.monitoring_unavailable && !d.restart_stalled);
    }

    #[test]
    fn continuous_heartbeats_for_six_hours_never_restart() {
        let t = Instant::now();
        let mut m = HealthMonitor::new(t);
        for s in (0..21600).step_by(2) {
            let d = observe(
                &mut m,
                t + Duration::from_secs(s),
                if s % 12 == 0 { "Heartbeat done" } else { "" },
            );
            assert!(
                !d.restart_stalled,
                "healthy core must survive beyond three minutes"
            );
            assert_eq!(d.state, LinkState::Online);
        }
    }
    #[test]
    fn unchanged_log_does_not_renew_health_or_mask_a_real_stall() {
        let t = Instant::now();
        let mut m = HealthMonitor::new(t);
        observe(&mut m, t, "Heartbeat done");
        for s in (2..=182).step_by(2) {
            let d = observe(&mut m, t + Duration::from_secs(s), "");
            assert_eq!(d.restart_stalled, s > 180);
            assert!(
                !d.stable,
                "one old heartbeat cannot establish sustained health"
            );
        }
    }
    #[test]
    fn core_retries_are_not_interrupted_and_ignored_timeouts_keep_online() {
        let t = Instant::now();
        let mut m = HealthMonitor::new(t);
        observe(&mut m, t, "Heartbeat done");
        assert_eq!(
            observe(
                &mut m,
                t + Duration::from_secs(2),
                "Heartbeat timeout, but ignored"
            )
            .state,
            LinkState::Online
        );
        for s in (4..600).step_by(2) {
            let d = observe(
                &mut m,
                t + Duration::from_secs(s),
                if s % 10 == 0 {
                    "Fatal error at UDP Process thread! Will try restart in 15 second(s)."
                } else {
                    ""
                },
            );
            assert!(!d.restart_stalled);
        }
        let d = observe(&mut m, t + Duration::from_secs(600), "Heartbeat done");
        assert_eq!(d.state, LinkState::Online);
        assert!(!d.stable);
    }
    #[test]
    fn scheduled_wait_survives_the_night_without_respawn() {
        let t = Instant::now();
        let mut m = HealthMonitor::new(t);
        observe(&mut m, t, "Will try reconnect at the next 7:00.");
        for s in (2..28800).step_by(2) {
            let d = observe(&mut m, t + Duration::from_secs(s), "");
            assert!(!d.restart_stalled);
            assert_eq!(d.state, LinkState::Waiting);
        }
        assert_eq!(
            observe(&mut m, t + Duration::from_secs(28800), "Heartbeat done").state,
            LinkState::Online
        );
    }
    #[test]
    fn sleep_and_log_read_failure_grant_recovery_time() {
        let t = Instant::now();
        let mut m = HealthMonitor::new(t);
        observe(&mut m, t, "Heartbeat done");
        assert!(!observe(&mut m, t + Duration::from_secs(3600), "").restart_stalled);
        for s in (3602..4000).step_by(2) {
            assert!(!m.observe(t + Duration::from_secs(s), None).restart_stalled);
        }
        assert!(!observe(&mut m, t + Duration::from_secs(4000), "").restart_stalled);
    }
    #[test]
    fn unhealthy_intervals_break_stable_health_streak() {
        let t = Instant::now();
        let mut m = HealthMonitor::new(t);
        for s in (0..120).step_by(2) {
            observe(&mut m, t + Duration::from_secs(s), "Heartbeat done");
        }
        assert!(!observe(&mut m, t + Duration::from_secs(120), "Send error").stable);
        assert!(!observe(&mut m, t + Duration::from_secs(122), "Heartbeat done").stable);
        for s in (124..=242).step_by(2) {
            let d = observe(&mut m, t + Duration::from_secs(s), "Heartbeat done");
            assert_eq!(d.stable, s == 242);
        }
    }
}
