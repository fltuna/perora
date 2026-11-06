//! Small motions a window draws itself, frame by frame: what CSS transitions and animations did in
//! the Tauri version (a fade, a few pixels of slide, a tick that bounces in). None for a user who
//! has the system move nothing.

use std::time::{Duration, Instant};

use gpui::Window;

/// CSS's timing functions, as cubic Bézier curves: `ease`, `ease-in`, `ease-out`.
pub const EASE: Curve = Curve(0.25, 0.1, 0.25, 1.0);
pub const EASE_IN: Curve = Curve(0.42, 0.0, 1.0, 1.0);
pub const EASE_OUT: Curve = Curve(0.0, 0.0, 0.58, 1.0);
pub const EASE_IN_OUT: Curve = Curve(0.42, 0.0, 0.58, 1.0);

/// A cubic Bézier curve from (0, 0) to (1, 1), by its two control points (as CSS has it).
#[derive(Debug, Clone, Copy)]
pub struct Curve(pub f32, pub f32, pub f32, pub f32);

impl Curve {
    /// How far along the motion is (0 to 1) at that share of its time.
    pub fn at(self, time: f32) -> f32 {
        let time = time.clamp(0.0, 1.0);
        if time == 0.0 || time == 1.0 {
            return time;
        }
        let Curve(x1, y1, x2, y2) = self;
        let along = |a: f32, b: f32, t: f32| {
            let u = 1.0 - t;
            3.0 * u * u * t * a + 3.0 * u * t * t * b + t * t * t
        };
        // The point of the curve that is that far across: halved down to it (the curve only goes
        // forward across).
        let (mut low, mut high) = (0.0_f32, 1.0_f32);
        for _ in 0..24 {
            let middle = (low + high) / 2.0;
            if along(x1, x2, middle) < time {
                low = middle;
            } else {
                high = middle;
            }
        }
        along(y1, y2, (low + high) / 2.0)
    }
}

/// How far along (0 to 1, by time) a motion is that began at `began`, starts `delay` after that and
/// lasts `length`.
pub fn share(began: Instant, delay: Duration, length: Duration) -> f32 {
    let elapsed = began.elapsed().saturating_sub(delay).as_secs_f32();
    (elapsed / length.as_secs_f32().max(f32::EPSILON)).min(1.0)
}

/// `share`, as a window draws it: while the motion is not over, the window is asked to draw the
/// next frame too.
pub fn along(began: Instant, delay: Duration, length: Duration, window: &mut Window) -> f32 {
    let done = share(began, delay, length);
    if done < 1.0 {
        window.request_animation_frame();
    }
    done
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_curve_goes_from_nothing_to_all() {
        for curve in [EASE, EASE_IN, EASE_OUT] {
            assert_eq!(curve.at(0.0), 0.0);
            assert_eq!(curve.at(1.0), 1.0);
            let mut before = 0.0;
            for step in 1..=20 {
                let now = curve.at(step as f32 / 20.0);
                assert!(now >= before);
                before = now;
            }
        }
        // Ease-out is quick at first, ease-in slow.
        assert!(EASE_OUT.at(0.25) > 0.25);
        assert!(EASE_IN.at(0.25) < 0.25);
        // A straight line is a straight line.
        assert!((Curve(0.0, 0.0, 1.0, 1.0).at(0.3) - 0.3).abs() < 0.001);
    }
}
