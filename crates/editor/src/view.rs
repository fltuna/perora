//! How the picture is looked at: how closely, and which part of it is in the middle. Once is all of
//! it (of what it is cut down to), fitted to its place and never larger than it is; more is a part of
//! it, as much larger; less is all of it, smaller, with room around it to begin drawing from.
//!
//! The arithmetic of `editor.tsx` (`lookAt`, `zoomAt`), so that the GPUI editor moves as the Tauri one
//! did. Places are in pixels of the image; sizes on screen in logical pixels.

use perora_core::edit::Place;

/// The closest the picture is looked at: a pixel of the image this many pixels of the screen.
pub const CLOSEST: f32 = 8.0;
/// The furthest: this part of the size it has fitted to its place (owner, 2026-10-14).
pub const FARTHEST: f32 = 0.25;
/// Looked at this closely or closer (pixels of the screen, not logical ones, to one of the image),
/// the pixels of the picture are shown as the squares they are.
pub const SQUARES_FROM: f32 = 3.0;

/// A part of the image: its top-left corner and its size, in pixels of the image.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Area {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

impl Area {
    pub fn new(x: f32, y: f32, width: f32, height: f32) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    pub fn right(&self) -> f32 {
        self.x + self.width
    }

    pub fn bottom(&self) -> f32 {
        self.y + self.height
    }

    pub fn contains(&self, at: Place) -> bool {
        at[0] >= self.x && at[0] <= self.right() && at[1] >= self.y && at[1] <= self.bottom()
    }

    /// Whether some of the other is in this one.
    pub fn meets(&self, other: &Area) -> bool {
        other.x < self.right()
            && other.right() > self.x
            && other.y < self.bottom()
            && other.bottom() > self.y
    }
}

/// What the user asked of the look: how closely (1 is all of it, fitted), and which place of the
/// picture is in the middle (none: the middle of the picture).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Looking {
    pub times: f32,
    pub middle: Option<Place>,
}

impl Default for Looking {
    fn default() -> Self {
        Self {
            times: 1.0,
            middle: None,
        }
    }
}

impl Looking {
    /// All of the picture, fitted, in the middle: as it is when the capture is shown.
    pub fn is_whole(&self) -> bool {
        self.times == 1.0 && self.middle.is_none()
    }
}

/// The look as it comes out in a place of a size.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Look {
    /// How many logical pixels of the screen one of the image is.
    pub scale: f32,
    /// How closely, as far as it goes.
    pub times: f32,
    /// What all of the place shows: the part of the image (reaching beyond the picture where it does
    /// not fill the place).
    pub part: Area,
    /// The place of the picture in the middle of the place: always somewhere on the picture, so that
    /// it is never out of sight however far it is moved (owner, 2026-10-14).
    pub middle: Place,
}

/// What the place shows, a part of the image, of `size` (logical pixels) on a screen of `dense`
/// pixels to a logical one, looked at as asked; `view` is the picture (all of the image, or what it
/// is cut down to).
pub fn look(size: (f32, f32), view: Area, looking: Looking, dense: f32) -> Look {
    let fit = fitted(size, view);
    let times = held_times(fit, looking.times);
    look_at(size, view, fit, times, looking.middle, dense)
}

/// How large all of the picture is when it fits in its place: never larger than it is.
fn fitted(size: (f32, f32), view: Area) -> f32 {
    if view.width <= 0.0 || view.height <= 0.0 {
        return 0.0;
    }
    let fit = 1f32.min(size.0 / view.width).min(size.1 / view.height);
    if fit.is_finite() && fit > 0.0 {
        fit
    } else {
        0.0
    }
}

/// How closely it can be looked at, from fitted: as close as a pixel of the image to `CLOSEST`
/// of the screen, at most.
fn closest(fit: f32) -> f32 {
    if fit > 0.0 {
        (CLOSEST / fit).max(1.0)
    } else {
        1.0
    }
}

fn held_times(fit: f32, times: f32) -> f32 {
    times.max(FARTHEST).min(closest(fit))
}

fn look_at(
    size: (f32, f32),
    view: Area,
    fit: f32,
    times: f32,
    middle: Option<Place>,
    dense: f32,
) -> Look {
    let by = fit * times;
    let middle = [
        middle
            .map_or(view.x + view.width / 2.0, |middle| middle[0])
            .clamp(view.x, view.right()),
        middle
            .map_or(view.y + view.height / 2.0, |middle| middle[1])
            .clamp(view.y, view.bottom()),
    ];
    if by == 0.0 {
        return Look {
            scale: 0.0,
            times,
            part: view,
            middle,
        };
    }
    let (width, height) = (size.0 / by, size.1 / by);
    // (The corner of the picture on a whole pixel of the screen: its pixels are not smudged across
    // two, and what is shown at a pixel to a pixel is the image. The place's own corner is on one.)
    let pixel = by * dense;
    let (x0, y0) = (middle[0] - width / 2.0, middle[1] - height / 2.0);
    Look {
        scale: by,
        times,
        part: Area::new(
            view.x - ((view.x - x0) * pixel).round() / pixel,
            view.y - ((view.y - y0) * pixel).round() / pixel,
            width,
            height,
        ),
        middle,
    }
}

impl Look {
    /// Where a place of the image is in the place it is shown in (logical pixels from its corner).
    pub fn on_screen(&self, at: Place) -> (f32, f32) {
        (
            (at[0] - self.part.x) * self.scale,
            (at[1] - self.part.y) * self.scale,
        )
    }

    /// The place of the image at a point of the place it is shown in.
    pub fn in_image(&self, x: f32, y: f32) -> Place {
        if self.scale <= 0.0 {
            return [self.part.x, self.part.y];
        }
        [self.part.x + x / self.scale, self.part.y + y / self.scale]
    }

    /// How closely it is looked at, as the window says it: a hundred is a pixel of the image to one
    /// of the screen.
    pub fn percent(&self, dense: f32) -> u32 {
        (self.scale * dense * 100.0).round().max(0.0) as u32
    }
}

/// The look after the wheel turned `lines` (as the system counts them: up, away from the user, is
/// more) with the pointer at `at` (logical pixels from the corner of the place): closer or from
/// further away, with the place under the pointer staying under it. Passing fitted, it stops there
/// once: the size all of it fits in is easy to come back to. None when it does not change.
pub fn wheeled(
    size: (f32, f32),
    view: Area,
    looking: Looking,
    dense: f32,
    at: (f32, f32),
    lines: f32,
) -> Option<Looking> {
    let fit = fitted(size, view);
    if fit == 0.0 || size.0 <= 0.0 || size.1 <= 0.0 {
        return None;
    }
    let times = held_times(fit, looking.times);
    let now = look_at(size, view, fit, times, looking.middle, dense);
    let (across, down) = (at.0 / size.0, at.1 / size.1);
    let under = [
        now.part.x + across * now.part.width,
        now.part.y + down * now.part.height,
    ];
    // (As a browser tells the wheel: a hundred pixels a notch, three lines.)
    let delta = -lines * (100.0 / 3.0);
    let mut next = held_times(fit, times * (-delta * 0.0015).exp());
    if (times > 1.0 && next < 1.0) || (times < 1.0 && next > 1.0) {
        next = 1.0;
    }
    if next == times {
        return None;
    }
    let then = look_at(size, view, fit, next, None, dense);
    Some(Looking {
        times: next,
        middle: Some([
            under[0] - (across - 0.5) * then.part.width,
            under[1] - (down - 0.5) * then.part.height,
        ]),
    })
}

/// The look while the picture is taken hold of and moved: it was `was` in the middle when the
/// pointer took hold at `from`, and the pointer is at `to` now (logical pixels).
pub fn moved(
    looking: Looking,
    was: Place,
    from: (f32, f32),
    to: (f32, f32),
    scale: f32,
) -> Looking {
    if scale <= 0.0 {
        return looking;
    }
    Looking {
        times: looking.times,
        middle: Some([
            was[0] - (to.0 - from.0) / scale,
            was[1] - (to.1 - from.1) / scale,
        ]),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const WIDE: Area = Area {
        x: 0.0,
        y: 0.0,
        width: 5120.0,
        height: 1440.0,
    };

    #[test]
    fn all_of_it_fits_and_is_never_larger_than_it_is() {
        let fitted = look((1168.0, 700.0), WIDE, Looking::default(), 1.0);
        assert!((fitted.scale - 1168.0 / 5120.0).abs() < 1e-6);
        assert_eq!(fitted.middle, [2560.0, 720.0]);
        // Small: as it is.
        let small = Area::new(0.0, 0.0, 200.0, 100.0);
        assert_eq!(
            look((1168.0, 700.0), small, Looking::default(), 1.5).scale,
            1.0
        );
    }

    #[test]
    fn it_is_looked_at_no_closer_and_no_further_than_that() {
        let size = (1168.0, 700.0);
        let close = look(
            size,
            WIDE,
            Looking {
                times: 1000.0,
                middle: None,
            },
            1.0,
        );
        assert!((close.scale - CLOSEST).abs() < 1e-4);
        let far = look(
            size,
            WIDE,
            Looking {
                times: 0.01,
                middle: None,
            },
            1.0,
        );
        assert!((far.times - FARTHEST).abs() < 1e-6);
    }

    #[test]
    fn the_middle_is_always_on_the_picture() {
        let moved = look(
            (1168.0, 700.0),
            WIDE,
            Looking {
                times: 4.0,
                middle: Some([-500.0, 9000.0]),
            },
            1.0,
        );
        assert_eq!(moved.middle, [0.0, 1440.0]);
    }

    #[test]
    fn the_corner_of_the_picture_is_on_a_whole_pixel() {
        for dense in [1.0, 1.25, 1.5, 2.0] {
            let seen = look(
                (1167.0, 703.0),
                WIDE,
                Looking {
                    times: 3.3,
                    middle: Some([1234.5, 678.9]),
                },
                dense,
            );
            let (x, y) = seen.on_screen([0.0, 0.0]);
            let device = (x * dense, y * dense);
            // (As whole as the arithmetic of floats lets it be.)
            assert!(
                (device.0 - device.0.round()).abs() < 0.02,
                "{dense}: {device:?}"
            );
            assert!(
                (device.1 - device.1.round()).abs() < 0.02,
                "{dense}: {device:?}"
            );
        }
    }

    #[test]
    fn the_wheel_keeps_the_place_under_the_pointer() {
        let size = (1168.0, 700.0);
        let at = (300.0, 200.0);
        let before = look(size, WIDE, Looking::default(), 1.0);
        let under = before.in_image(at.0, at.1);
        let looking = wheeled(size, WIDE, Looking::default(), 1.0, at, 3.0).unwrap();
        assert!(looking.times > 1.0);
        // (Within a pixel of the screen: the corner of the picture goes to a whole one.)
        let after = look(size, WIDE, looking, 1.0);
        let still = after.on_screen(under);
        assert!(
            (still.0 - at.0).abs() <= 1.0 && (still.1 - at.1).abs() <= 1.0,
            "{still:?}"
        );
    }

    #[test]
    fn passing_fitted_it_stops_there_once() {
        let size = (1168.0, 700.0);
        let close = Looking {
            times: 1.05,
            middle: None,
        };
        let back = wheeled(size, WIDE, close, 1.0, (10.0, 10.0), -3.0).unwrap();
        assert_eq!(back.times, 1.0);
        let further = wheeled(size, WIDE, back, 1.0, (10.0, 10.0), -3.0).unwrap();
        assert!(further.times < 1.0);
        // As far as it goes: no change.
        let farthest = Looking {
            times: FARTHEST,
            middle: None,
        };
        assert_eq!(wheeled(size, WIDE, farthest, 1.0, (10.0, 10.0), -3.0), None);
    }

    #[test]
    fn taking_hold_of_it_moves_it_with_the_pointer() {
        let looking = Looking {
            times: 2.0,
            middle: Some([1000.0, 500.0]),
        };
        let moved = moved(looking, [1000.0, 500.0], (100.0, 100.0), (150.0, 80.0), 0.5);
        assert_eq!(moved.middle, Some([900.0, 540.0]));
    }

    #[test]
    fn a_hundred_percent_is_a_pixel_to_a_pixel() {
        let small = Area::new(0.0, 0.0, 200.0, 100.0);
        assert_eq!(
            look((1168.0, 700.0), small, Looking::default(), 1.5).percent(1.5),
            150
        );
    }
}
