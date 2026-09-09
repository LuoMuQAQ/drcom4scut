//! Short UI tweens for toggles and the adapter menu.

use std::time::{Duration, Instant};

use windows::Win32::Foundation::COLORREF;

#[derive(Clone, Copy)]
pub struct Anim {
    from: f32,
    to: f32,
    start: Instant,
    duration: Duration,
    value: f32,
    running: bool,
}

impl Anim {
    pub fn snap(value: f32) -> Self {
        Self {
            from: value,
            to: value,
            start: Instant::now(),
            duration: Duration::ZERO,
            value,
            running: false,
        }
    }

    pub fn go(from: f32, to: f32, ms: u32) -> Self {
        Self::at(from, to, ms, Instant::now())
    }

    fn at(from: f32, to: f32, ms: u32, now: Instant) -> Self {
        if ms == 0 || from == to {
            return Self::snap(to);
        }
        Self {
            from,
            to,
            start: now,
            // A reversal travels only the remaining distance, without a long tail.
            duration: Duration::from_secs_f32(ms as f32 / 1000.0 * (to - from).abs()),
            value: from,
            running: true,
        }
    }

    pub fn value(&self) -> f32 {
        self.value
    }

    /// Sample once per frame. Every surface paints the same presentation state.
    pub fn advance(&mut self, now: Instant) -> bool {
        if !self.running {
            return false;
        }
        let t = (now.saturating_duration_since(self.start).as_secs_f32()
            / self.duration.as_secs_f32())
        .clamp(0.0, 1.0);
        self.value = lerp(self.from, self.to, ease_out_cubic(t));
        self.running = t < 1.0;
        true
    }

    pub fn done(&self) -> bool {
        !self.running
    }
}

pub fn ease_out_cubic(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    1.0 - (1.0 - t).powi(3)
}

pub fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t.clamp(0.0, 1.0)
}

pub fn lerp_color(a: COLORREF, b: COLORREF, t: f32) -> COLORREF {
    let t = t.clamp(0.0, 1.0);
    let mix = |shift: u32| {
        let x = ((a.0 >> shift) & 0xFF) as f32;
        let y = ((b.0 >> shift) & 0xFF) as f32;
        (x + (y - x) * t).round() as u32
    };
    COLORREF(mix(0) | (mix(8) << 8) | (mix(16) << 16))
}

pub fn bool01(v: bool) -> f32 {
    if v {
        1.0
    } else {
        0.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ease_out_cubic_bounds_and_fast_start() {
        assert_eq!(ease_out_cubic(0.0), 0.0);
        assert_eq!(ease_out_cubic(1.0), 1.0);
        assert!(ease_out_cubic(0.5) > 0.8);
    }

    #[test]
    fn lerp_and_color_mix() {
        assert!((lerp(0.0, 10.0, 0.5) - 5.0).abs() < 1e-5);
        let c = lerp_color(COLORREF(0x00000000), COLORREF(0x00FFFFFF), 0.5);
        assert_eq!(c.0 & 0xFF, 128);
        assert_eq!((c.0 >> 8) & 0xFF, 128);
        assert_eq!((c.0 >> 16) & 0xFF, 128);
    }

    #[test]
    fn snap_holds_target() {
        let a = Anim::snap(1.0);
        assert!((a.value() - 1.0).abs() < 1e-6);
        assert!(a.done());
    }

    #[test]
    fn long_tween_starts_near_origin() {
        let a = Anim::go(0.0, 1.0, 10_000);
        assert!(a.value() < 0.2);
        assert!(!a.done());
    }

    #[test]
    fn frames_are_stable_and_finish_at_the_exact_endpoint() {
        let now = Instant::now();
        let mut a = Anim::at(0.0, 1.0, 200, now);
        assert_eq!(a.value(), 0.0); // No artificially skipped first frame.
        a.advance(now + Duration::from_millis(50));
        assert_eq!(a.value(), ease_out_cubic(0.25));
        assert_eq!(a.value(), a.value());
        assert!(!a.done());
        a.advance(now + Duration::from_secs(1)); // Delayed message still settles.
        assert_eq!(a.value(), 1.0);
        assert!(a.done());
        assert!(!a.advance(now + Duration::from_secs(2)));
    }

    #[test]
    fn reverse_starts_at_presented_position_and_finishes_sooner() {
        let now = Instant::now();
        let mut a = Anim::at(0.0, 1.0, 200, now);
        a.advance(now + Duration::from_millis(40));
        let shown = a.value();
        let mut reverse = Anim::at(shown, 0.0, 200, now);
        assert_eq!(reverse.value(), shown);
        reverse.advance(now + Duration::from_millis(20));
        assert!(reverse.value() < shown);
        reverse.advance(now + Duration::from_millis(100));
        assert_eq!(reverse.value(), 0.0);
        assert!(reverse.done());
        assert!(Anim::go(1.0, 1.0, 200).done());
        assert_eq!(Anim::go(0.0, 1.0, 0).value(), 1.0);
    }
}
