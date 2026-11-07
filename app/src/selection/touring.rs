//! The tour of the selection screen (P6 of docs/gpui-rewrite.md; the Tauri version's `overlay.tsx`):
//! the modes, the bar going away and the part of the screen that brings it back, what a click on a
//! window and a drag do in the smart mode, and how to give up. Only on the monitor with the bar;
//! while it is on, nothing is chosen on any monitor, the bar stays, and the list of windows is not
//! there. It comes once on its own, a moment after the screen first comes up; and again from the
//! "i" at the end of the bar.

use std::time::Duration;

use gpui::{
    App, BorderStyle, Bounds, Corners, Global, Hsla, IntoElement, ParentElement, Pixels, Styled,
    canvas, div, point, px, quad, rgba, size,
};
use perora_core::geometry::Rect;
use perora_core::language::Language;
use perora_core::platform::{Monitor, Window as Listed};
use perora_ui::motion::{EASE_IN_OUT, EASE_OUT};
use perora_ui::tour::{At, Look, Step, Tour};

/// A moment after the screen came up, so that it is seen to come first.
pub const COMES_AFTER: Duration = Duration::from_millis(400);
/// The frame dragged open, again and again; the click on a window, again and again.
const DRAGGED: Duration = Duration::from_millis(2600);
const CLICKED: Duration = Duration::from_millis(1600);

/// What a step shows on the screen, under the tour.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Shown {
    Nothing,
    /// The part of the screen that brings the bar back.
    Zone(Bounds<Pixels>),
    /// A click on a window: on one of the screen's, or on one drawn for the purpose (`sample`).
    Click {
        frame: Bounds<Pixels>,
        sample: bool,
    },
    /// A frame dragged open, with its size in pixels of the screen (`scale` of them to one of the
    /// window).
    Drag {
        frame: Bounds<Pixels>,
        scale: f32,
    },
}

pub struct Touring {
    pub tour: Tour,
    pub shown: Vec<Shown>,
}

/// The tour of the selection screen was seen since Perora started (before the settings say so).
#[derive(Default)]
struct Toured(bool);

impl Global for Toured {}

/// Whether the tour was seen: the settings say so, or it was, since Perora started.
pub fn seen(cx: &mut App) -> bool {
    cx.default_global::<Toured>().0
        || crate::state::Perora::get(cx)
            .settings()
            .tours_seen
            .contains(&perora_core::settings::Tour::Capture)
}

/// The tour was seen (to the end, or put away): it does not come on its own again.
pub fn saw(cx: &mut App) {
    let toured = cx.default_global::<Toured>();
    if toured.0 {
        return;
    }
    toured.0 = true;
    let perora = crate::state::Perora::get(cx);
    if let Some(path) = perora.settings_path.clone() {
        perora_core::settings::tour_seen(&path, &perora.settings, "capture");
    }
}

/// The steps, on the monitor with the bar (`home`, whose windows are `windows`, front to back), in
/// the user's language.
pub fn steps(
    home: &Monitor,
    windows: &[Listed],
    bar_seconds: f64,
    zone: perora_core::settings::Zone,
    language: &Language,
) -> Touring {
    let t = |key: &str| language.text(key, &[]);
    let scale = home.scale as f32;
    let (wide, high) = (
        home.bounds.width as f32 / scale,
        home.bounds.height as f32 / scale,
    );
    let place = |left: f32, top: f32, width: f32, height: f32| {
        Bounds::new(point(px(left), px(top)), size(px(width), px(height)))
    };
    let mut steps = vec![(
        Step::new(At::Mark("modes"), t("tour.capture.modes")),
        Shown::Nothing,
    )];
    // The bar slides away (when it does), and comes back with the pointer in the part set for that:
    // the part is shown, and where to change both (owner, 2026-11-02: 「チュートリアルに自動で隠れる
    // けど上に行けば出せるよーってのと同時に枠だそう。で、設定からいじれるよーってのも解説しておこ」).
    if bar_seconds > 0.0 {
        // (A strip of a few pixels at the top is shown as one.)
        let reveal = place(
            (zone.x as f32 * wide).round(),
            (zone.y as f32 * high).round(),
            (zone.width as f32 * wide).round(),
            (zone.height as f32 * high).round().max(4.0),
        );
        steps.push((
            Step::new(
                At::Place(reveal),
                language.text(
                    "tour.capture.barHide",
                    &[("seconds", &bar_seconds.to_string())],
                ),
            ),
            Shown::Zone(reveal),
        ));
    }
    let example = example(home.bounds, windows);
    let frame = match example {
        Some(window) => place(
            (window.x - home.bounds.x) as f32 / scale,
            (window.y - home.bounds.y) as f32 / scale,
            window.width as f32 / scale,
            window.height as f32 / scale,
        ),
        None => place(
            (wide * 0.28).round(),
            (high * 0.3).round(),
            (wide * 0.44).round(),
            (high * 0.44).round(),
        ),
    };
    steps.push((
        Step::new(At::Place(frame), t("tour.capture.window")),
        Shown::Click {
            frame,
            sample: example.is_none(),
        },
    ));
    let dragged = place(
        (wide * 0.32).round(),
        (high * 0.36).round(),
        (wide * 0.34).round(),
        (high * 0.3).round(),
    );
    steps.push((
        Step::new(At::Place(dragged), t("tour.capture.drag")).look(Look::Drag),
        Shown::Drag {
            frame: dragged,
            scale,
        },
    ));
    steps.push((
        Step::new(At::Mark("cancel"), t("tour.capture.cancel")),
        Shown::Nothing,
    ));
    steps.push((
        Step::new(At::Mark("guide"), t("tour.again")),
        Shown::Nothing,
    ));
    let (steps, shown) = steps.into_iter().unzip();
    Touring {
        tour: Tour::new(steps, perora_core::platform::animations_enabled(), false).square(),
        shown,
    }
}

/// The window the tour shows a click on: the first, front to back, that is on this monitor (most of
/// it), neither small nor as large as the monitor, and in sight where it is clicked (no other in front
/// of it there: a click there would be on that one).
fn example(area: Rect, windows: &[Listed]) -> Option<Rect> {
    let inside = |one: &Rect, x: i32, y: i32| {
        x >= one.x && x < one.x + one.width as i32 && y >= one.y && y < one.y + one.height as i32
    };
    for listed in windows {
        let one = listed.bounds;
        let (left, top) = (one.x.max(area.x), one.y.max(area.y));
        let right = (one.x + one.width as i32).min(area.x + area.width as i32);
        let bottom = (one.y + one.height as i32).min(area.y + area.height as i32);
        if right <= left || bottom <= top {
            continue;
        }
        let seen = (right - left) as f64 * (bottom - top) as f64;
        if seen < 0.8 * one.width as f64 * one.height as f64 {
            continue;
        }
        let across = one.width as f64 / area.width as f64;
        let down = one.height as f64 / area.height as f64;
        if across < 0.15 || down < 0.15 || (across > 0.92 && down > 0.92) {
            continue;
        }
        let (x, y) = (one.x + one.width as i32 / 2, one.y + one.height as i32 / 2);
        if windows
            .iter()
            .find(|window| inside(&window.bounds, x, y))
            .map(|window| window.bounds)
            != Some(one)
        {
            continue;
        }
        return Some(one);
    }
    None
}

/// What the step on now shows on the screen, under the tour: the part that brings the bar back, a
/// window drawn for the purpose with a click on it, the pointer dragging a frame open.
pub fn art(
    touring: &Touring,
    language: &Language,
    theme: &perora_ui::Theme,
) -> Option<gpui::AnyElement> {
    let shown = touring.shown.get(touring.tour.at()).copied()?;
    let began = touring.tour.step_began();
    let moves = perora_core::platform::animations_enabled();
    // (The selection screen is always dark.)
    let accent: Hsla = theme.accent_dark;
    Some(match shown {
        Shown::Nothing => return None,
        Shown::Zone(zone) => div()
            .absolute()
            .left(zone.origin.x)
            .top(zone.origin.y)
            .w(zone.size.width)
            .h(zone.size.height)
            .bg(accent.opacity(0.35))
            .border_1()
            .border_color(accent)
            .into_any_element(),
        Shown::Click { frame, sample } => {
            let middle = frame.center();
            div()
                .absolute()
                .inset_0()
                .when_sample(sample, frame, language)
                .child(
                    canvas(
                        |_, _, _| {},
                        move |_, _, window, _| {
                            if moves {
                                window.request_animation_frame();
                                // A ring that comes out of the pointer's tip, again and again.
                                let cycle =
                                    (began.elapsed().as_secs_f32() / CLICKED.as_secs_f32()).fract();
                                let grown = EASE_OUT.at(((cycle - 0.3) / 0.5).clamp(0.0, 1.0));
                                let opacity = if cycle < 0.3 {
                                    0.0
                                } else if cycle < 0.4 {
                                    (cycle - 0.3) / 0.1
                                } else {
                                    (1.0 - (cycle - 0.4) / 0.4).clamp(0.0, 1.0)
                                };
                                let side = 24.0 * (0.3 + 1.1 * grown);
                                let ring = Bounds::new(
                                    middle - point(px(side / 2.0), px(side / 2.0)),
                                    size(px(side), px(side)),
                                );
                                window.paint_quad(quad(
                                    ring,
                                    Corners::all(px(side / 2.0)),
                                    gpui::transparent_black(),
                                    px(2.0),
                                    accent.opacity(opacity),
                                    BorderStyle::Solid,
                                ));
                            }
                            perora_ui::hand::pointer(middle, window);
                        },
                    )
                    .absolute()
                    .inset_0(),
                )
                .into_any_element()
        }
        Shown::Drag { frame, scale } => {
            // The pointer from the corner to the far one as the frame opens (the tour's own frame
            // goes with it), and the size as the screen says it once it is most of the way.
            let open = if moves {
                let cycle = (began.elapsed().as_secs_f32() / DRAGGED.as_secs_f32()).fract();
                EASE_IN_OUT.at(((cycle - 0.12) / 0.5).clamp(0.0, 1.0))
            } else {
                1.0
            };
            let said = if moves {
                let cycle = (began.elapsed().as_secs_f32() / DRAGGED.as_secs_f32()).fract();
                ((cycle - 0.58) / 0.08).clamp(0.0, 1.0)
            } else {
                1.0
            };
            let at = frame.origin + point(frame.size.width * open, frame.size.height * open);
            let words = format!(
                "{} × {}",
                (frame.size.width.as_f32() * scale).round(),
                (frame.size.height.as_f32() * scale).round()
            );
            div()
                .absolute()
                .inset_0()
                .child(
                    canvas(
                        |_, _, _| {},
                        move |_, _, window, _| {
                            if moves {
                                window.request_animation_frame();
                            }
                            perora_ui::hand::pointer(at, window);
                        },
                    )
                    .absolute()
                    .inset_0(),
                )
                .child(
                    div()
                        .absolute()
                        .right(px(0.0))
                        .left(at.x - px(200.0))
                        .top(at.y + px(26.0))
                        .w(px(200.0))
                        .flex()
                        .justify_end()
                        .opacity(said)
                        .child(
                            div()
                                .px(px(6.0))
                                .py(px(2.0))
                                .text_size(px(12.0))
                                .whitespace_nowrap()
                                .text_color(gpui::white())
                                .bg(rgba(0x0000_00bf))
                                .child(words),
                        ),
                )
                .into_any_element()
        }
    })
}

/// A window drawn for the tour, where there is none on the screen to show it on.
trait Sample {
    fn when_sample(self, sample: bool, frame: Bounds<Pixels>, language: &Language) -> Self;
}

impl Sample for gpui::Div {
    fn when_sample(self, sample: bool, frame: Bounds<Pixels>, language: &Language) -> Self {
        if !sample {
            return self;
        }
        let bar = |width: f32| {
            div()
                .h(px(10.0))
                .w(gpui::relative(width))
                .rounded(px(5.0))
                .bg(gpui::rgb(0xd6d6d6))
        };
        self.child(
            div()
                .absolute()
                .left(frame.origin.x)
                .top(frame.origin.y)
                .w(frame.size.width)
                .h(frame.size.height)
                .flex()
                .flex_col()
                .overflow_hidden()
                .bg(gpui::rgb(0xf3f3f3))
                .border_1()
                .border_color(gpui::rgb(0x8a8a8a))
                .rounded(px(8.0))
                .shadow(vec![gpui::BoxShadow {
                    color: gpui::hsla(0.0, 0.0, 0.0, 0.4),
                    offset: point(px(0.0), px(12.0)),
                    blur_radius: px(32.0),
                    spread_radius: px(0.0),
                    inset: false,
                }])
                .child(
                    div()
                        .h(px(32.0))
                        .px(px(14.0))
                        .flex()
                        .items_center()
                        .justify_between()
                        .text_size(px(12.0))
                        .text_color(gpui::rgb(0x1a1a1a))
                        .bg(gpui::rgb(0xe8e8e8))
                        .child(language.text("tour.capture.sample", &[]))
                        .child(
                            div()
                                .font_family("Segoe Fluent Icons")
                                .text_size(px(10.0))
                                .child("\u{E921}  \u{E922}  \u{E8BB}"),
                        ),
                )
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(12.0))
                        .p(px(20.0))
                        .child(bar(1.0))
                        .child(bar(0.7))
                        .child(bar(0.45)),
                ),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use perora_core::platform::WindowId;

    fn listed(x: i32, y: i32, width: u32, height: u32) -> Listed {
        Listed {
            id: WindowId(0),
            title: String::new(),
            bounds: Rect::new(x, y, width, height),
        }
    }

    #[test]
    fn the_window_shown_is_one_fit_to_click() {
        let monitor = Rect::new(0, 0, 1920, 1080);
        // Too small, one half off it, and (behind the one shown) all of the monitor: none of those.
        let windows = [
            listed(10, 10, 100, 100),
            listed(1700, 100, 800, 600),
            listed(300, 200, 800, 600),
            listed(0, 0, 1920, 1080),
        ];
        assert_eq!(
            example(monitor, &windows),
            Some(Rect::new(300, 200, 800, 600))
        );
        // The first fit, front to back (the one behind it has its middle under it).
        let covered = [listed(500, 300, 500, 400), listed(300, 200, 800, 600)];
        assert_eq!(
            example(monitor, &covered),
            Some(Rect::new(500, 300, 500, 400))
        );
        assert_eq!(example(monitor, &[]), None);
    }
}
