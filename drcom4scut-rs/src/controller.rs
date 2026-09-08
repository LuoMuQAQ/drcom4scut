//! 连接编排：重连退避策略。
//!
//! 数值与语义严格对齐 .NET 版 `Services.cs:412-455`：
//! 退避序列 [2s, 5s, 15s, 30s, 60s]；启动记录窗口 10 分钟；
//! 失败次数 > 5 或窗口内启动 > 5 次进入 5 分钟冷却并清零；
//! 用户点击连接 / 手动断开 / 连续健康 2 分钟时重置。

use std::time::{Duration, Instant};

const DELAYS: [Duration; 5] = [
    Duration::from_secs(2),
    Duration::from_secs(5),
    Duration::from_secs(15),
    Duration::from_secs(30),
    Duration::from_secs(60),
];
const LAUNCH_WINDOW: Duration = Duration::from_secs(600);
const COOLDOWN: Duration = Duration::from_secs(300);
const MAX_FAILURES: u32 = 5;
const MAX_LAUNCHES_IN_WINDOW: usize = 5;

#[derive(Debug, Default)]
pub struct ReconnectBackoff {
    failures: u32,
    launches: Vec<Instant>,
    next_attempt: Option<Instant>,
    cooldown_until: Option<Instant>,
}

impl ReconnectBackoff {
    pub fn new() -> Self {
        Self::default()
    }

    /// 记录一次核心启动（成功拉起进程时调用）。
    pub fn record_launch(&mut self, now: Instant) {
        self.launches.push(now);
        let cutoff = now - LAUNCH_WINDOW;
        self.launches.retain(|t| *t >= cutoff);
    }

    /// 记录一次失败，返回下次尝试应等待的时长。
    pub fn record_failure(&mut self, now: Instant) -> Duration {
        self.failures += 1;
        let cutoff = now - LAUNCH_WINDOW;
        self.launches.retain(|t| *t >= cutoff);
        if self.failures > MAX_FAILURES || self.launches.len() > MAX_LAUNCHES_IN_WINDOW {
            self.failures = 0;
            self.launches.clear();
            let until = now + COOLDOWN;
            self.next_attempt = Some(until);
            self.cooldown_until = Some(until);
            return COOLDOWN;
        }
        let delay = DELAYS[(self.failures as usize - 1).min(DELAYS.len() - 1)];
        self.next_attempt = Some(now + delay);
        delay
    }

    /// 现在是否允许尝试启动。
    pub fn can_attempt(&self, now: Instant) -> bool {
        self.next_attempt.map_or(true, |t| now >= t)
            && self.cooldown_until.map_or(true, |t| now >= t)
    }

    /// 是否处于冷却期（UI 用于显示「暂停重试」而非「等待重连」）。
    pub fn in_cooldown(&self, now: Instant) -> bool {
        self.cooldown_until.map_or(false, |t| now < t)
    }

    /// 距下次尝试的剩余秒数（至少 1，用于「约 N 秒后重试」展示）。
    pub fn seconds_until_next_attempt(&self, now: Instant) -> Option<u64> {
        self.next_attempt
            .filter(|t| *t > now)
            .map(|t| (t - now).as_secs().max(1))
    }

    /// 用户点击连接 / 手动断开 / 连续健康 2 分钟时重置。
    pub fn reset(&mut self) {
        self.failures = 0;
        self.launches.clear();
        self.next_attempt = None;
        self.cooldown_until = None;
    }

    #[cfg(test)]
    fn state(&self) -> (u32, usize) {
        (self.failures, self.launches.len())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_failure_waits_two_seconds() {
        let mut b = ReconnectBackoff::new();
        let t0 = Instant::now();
        assert_eq!(b.record_failure(t0), Duration::from_secs(2));
        assert!(!b.can_attempt(t0));
        assert!(b.can_attempt(t0 + Duration::from_secs(2)));
    }

    #[test]
    fn delays_advance_through_sequence() {
        let mut b = ReconnectBackoff::new();
        let t0 = Instant::now();
        let mut t = t0;
        let expect = [2u64, 5, 15, 30, 60];
        for (i, secs) in expect.iter().enumerate() {
            assert_eq!(
                b.record_failure(t),
                Duration::from_secs(*secs),
                "第 {} 次失败",
                i + 1
            );
            // 立即再失败（不等待）也要按序列推进。
            t += Duration::from_secs(*secs);
        }
        // 第 6 次失败 > 5 → 冷却 5 分钟并清零。
        assert_eq!(b.record_failure(t), Duration::from_secs(300));
        assert!(b.in_cooldown(t));
        assert_eq!(b.state(), (0, 0));
    }

    #[test]
    fn launch_burst_triggers_cooldown() {
        let mut b = ReconnectBackoff::new();
        let t0 = Instant::now();
        // 10 分钟窗口内启动 6 次（> 5）。
        for i in 0..6 {
            b.record_launch(t0 + Duration::from_secs(i * 30));
        }
        assert_eq!(b.record_failure(t0), Duration::from_secs(300));
        assert!(b.in_cooldown(t0));
    }

    #[test]
    fn old_launches_fall_out_of_window() {
        let mut b = ReconnectBackoff::new();
        let t0 = Instant::now();
        for i in 0..6 {
            b.record_launch(t0 - Duration::from_secs(700 - i * 30));
        }
        // 全部在窗口外，只剩本次 → 不冷却。
        b.record_launch(t0);
        assert_eq!(b.record_failure(t0), Duration::from_secs(2));
        assert!(!b.in_cooldown(t0));
    }

    #[test]
    fn reset_clears_everything() {
        let mut b = ReconnectBackoff::new();
        let t0 = Instant::now();
        b.record_launch(t0);
        b.record_failure(t0);
        assert!(!b.can_attempt(t0));
        b.reset();
        assert!(b.can_attempt(t0));
        assert_eq!(b.state(), (0, 0));
        assert!(!b.in_cooldown(t0));
    }
}
