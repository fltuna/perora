//! A slider: a value between two ends, in steps, set by pressing on it and dragging (the Tauri
//! version's `<input type="range">`), in the look of Windows 11: a thin track, filled in the accent
//! colour up to the value, and a round thumb with the accent in its middle. Or the hues, one after
//! another, with a ring for a thumb (`hues`: where a colour is mixed).
//!
//! The pointer is followed while the button is held, also beside the slider and outside the window:
//! every move of the window is heard (GPUI has no capture of the pointer), and only the one that
//! began on the slider is taken (`follow`).

use std::rc::Rc;

use gpui::{
    App, Bounds, DispatchPhase, ElementId, Entity, Hitbox, Hsla, IntoElement, MouseButton,
    MouseDownEvent, MouseMoveEvent, MouseUpEvent, Pixels, Point, RenderOnce, Styled, Window,
    canvas, fill, linear_color_stop, linear_gradient, point, px, size,
};

use crate::Theme;

/// How wide the thumb is, and the track (of the hues: their thumb and their bar).
const THUMB: f32 = 20.0;
const TRACK: f32 = 4.0;
const HUES_THUMB: f32 = 14.0;
const HUES_TRACK: f32 = 12.0;

#[derive(IntoElement)]
pub struct Slider {
    id: ElementId,
    value: f32,
    least: f32,
    most: f32,
    step: f32,
    enabled: bool,
    /// Not to be pressed, whatever it looks like (on its way out).
    inert: bool,
    hues: bool,
    on_change: Option<OnChange>,
}

type OnChange = Rc<dyn Fn(f32, &mut Window, &mut App)>;

/// What a press, and the pointer moving with it held, is told to (where the pointer is).
pub(crate) type Follow = Rc<dyn Fn(Point<Pixels>, &mut Window, &mut App)>;

impl Slider {
    pub fn new(id: impl Into<ElementId>, value: f32, least: f32, most: f32) -> Self {
        Self {
            id: id.into(),
            value,
            least,
            most,
            step: 1.0,
            enabled: true,
            inert: false,
            hues: false,
            on_change: None,
        }
    }

    /// Values go by this much (1 unless said).
    pub fn step(mut self, step: f32) -> Self {
        self.step = step;
        self
    }

    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// Not to be pressed, while looking as it does.
    pub fn inert(mut self, inert: bool) -> Self {
        self.inert = inert;
        self
    }

    /// The hues, red round to red, for a value from 0 to 360.
    pub fn hues(mut self) -> Self {
        self.hues = true;
        self
    }

    /// Told of each value the slider is moved to (not again of the one it is at).
    pub fn on_change(mut self, on_change: impl Fn(f32, &mut Window, &mut App) + 'static) -> Self {
        self.on_change = Some(Rc::new(on_change));
        self
    }
}

/// The value at `x` across a track from `left`, `wide` across (logical pixels), to the nearest step,
/// with a thumb `thumb` across (the ends of the track are where its middle can go).
fn value_at(x: f32, left: f32, wide: f32, thumb: f32, (least, most, step): (f32, f32, f32)) -> f32 {
    let room = (wide - thumb).max(1.0);
    let share = ((x - left - thumb / 2.0) / room).clamp(0.0, 1.0);
    let steps = ((most - least) * share / step).round();
    (least + steps * step).clamp(least.min(most), most.max(least))
}

/// How far along the track a value is (0 to 1).
fn share_of(value: f32, least: f32, most: f32) -> f32 {
    if most == least {
        return 0.0;
    }
    ((value - least) / (most - least)).clamp(0.0, 1.0)
}

/// Follows the pointer from a press on `hitbox` for as long as the left button is held: `held` says
/// whether it is (kept across frames), and `to` is told where the pointer is at the press and at
/// each move meanwhile.
pub(crate) fn follow(window: &mut Window, hitbox: Hitbox, held: Entity<bool>, to: Follow) {
    let down = to.clone();
    let pressed = held.clone();
    window.on_mouse_event(move |event: &MouseDownEvent, phase, window, cx| {
        if phase == DispatchPhase::Bubble
            && event.button == MouseButton::Left
            && hitbox.is_hovered(window)
        {
            cx.stop_propagation();
            pressed.update(cx, |held, _| *held = true);
            down(event.position, window, cx);
        }
    });
    let moved = held.clone();
    window.on_mouse_event(move |event: &MouseMoveEvent, phase, window, cx| {
        if phase != DispatchPhase::Bubble || !*moved.read(cx) {
            return;
        }
        if event.pressed_button == Some(MouseButton::Left) {
            to(event.position, window, cx);
        } else {
            // (Let go where the window did not hear it.)
            moved.update(cx, |held, _| *held = false);
        }
    });
    window.on_mouse_event(move |event: &MouseUpEvent, phase, _, cx| {
        if phase == DispatchPhase::Bubble && event.button == MouseButton::Left && *held.read(cx) {
            held.update(cx, |held, _| *held = false);
        }
    });
}

impl RenderOnce for Slider {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = Theme::get(cx).clone();
        let held = window.use_keyed_state(self.id.clone(), cx, |_, _| false);
        // The value last given while the thumb is held.
        let last = window.use_keyed_state((self.id.clone(), "last"), cx, |_, _| None::<f32>);
        let Slider {
            value,
            least,
            most,
            step,
            enabled,
            inert,
            hues,
            on_change,
            ..
        } = self;
        let thumb = if hues { HUES_THUMB } else { THUMB };
        canvas(
            |bounds, window, _| window.insert_hitbox(bounds, gpui::HitboxBehavior::Normal),
            move |bounds, hitbox, window, cx| {
                let share = share_of(value, least, most);
                if hues {
                    paint_hues(window, bounds, share);
                } else {
                    paint(window, bounds, share, enabled, &theme);
                }
                if !enabled || inert {
                    return;
                }
                if !*held.read(cx) {
                    last.update(cx, |last, _| *last = None);
                }
                let (left, wide) = (bounds.origin.x.as_f32(), bounds.size.width.as_f32());
                let to: Follow = Rc::new(move |position, window, cx| {
                    let to = value_at(position.x.as_f32(), left, wide, thumb, (least, most, step));
                    let before = last.read(cx).unwrap_or(value);
                    last.update(cx, |last, _| *last = Some(to));
                    if to != before
                        && let Some(on_change) = &on_change
                    {
                        on_change(to, window, cx);
                    }
                });
                follow(window, hitbox, held, to);
            },
        )
        .h(px(if hues { HUES_TRACK + 2.0 } else { THUMB }))
        .w_full()
        .min_w(px(thumb * 2.0))
    }
}

fn paint(window: &mut Window, bounds: Bounds<Pixels>, share: f32, enabled: bool, theme: &Theme) {
    let (left, top) = (bounds.origin.x.as_f32(), bounds.origin.y.as_f32());
    let wide = bounds.size.width.as_f32();
    let middle = top + bounds.size.height.as_f32() / 2.0;
    let thumb = left + THUMB / 2.0 + (wide - THUMB).max(0.0) * share;
    let faded = |colour: Hsla| {
        if enabled {
            colour
        } else {
            Hsla {
                a: colour.a * 0.4,
                ..colour
            }
        }
    };
    let track = |from: f32, to: f32, colour: Hsla, window: &mut Window| {
        if to <= from {
            return;
        }
        window.paint_quad(
            fill(
                Bounds::new(
                    point(px(from), px(middle - TRACK / 2.0)),
                    size(px(to - from), px(TRACK)),
                ),
                faded(colour),
            )
            .corner_radii(px(TRACK / 2.0)),
        );
    };
    let rest = Hsla {
        a: 0.55,
        ..theme.muted
    };
    track(left + 2.0, thumb, theme.accent, window);
    track(thumb, left + wide - 2.0, rest, window);
    disc(
        window,
        (thumb, middle),
        THUMB,
        theme.surface_hover,
        Some(theme.border),
    );
    disc(window, (thumb, middle), 12.0, faded(theme.accent), None);
}

/// A disc `across` wide around `middle`, with an edge of 1 pixel.
fn disc(window: &mut Window, middle: (f32, f32), across: f32, colour: Hsla, edge: Option<Hsla>) {
    let mut quad = fill(
        Bounds::new(
            point(px(middle.0 - across / 2.0), px(middle.1 - across / 2.0)),
            size(px(across), px(across)),
        ),
        colour,
    )
    .corner_radii(px(across / 2.0));
    if let Some(edge) = edge {
        quad = quad.border_widths(px(1.0)).border_color(edge);
    }
    window.paint_quad(quad);
}

/// The colour of a hue (0 to 360), as strong and light as it is.
pub fn hue(degrees: f32) -> Hsla {
    gpui::hsla((degrees / 360.0).rem_euclid(1.0), 1.0, 0.5, 1.0)
}

/// The hues, red round to red (`linear-gradient(to right, #f00, #ff0, #0f0, #0ff, #00f, #f0f,
/// #f00)`), and a ring where the value is.
fn paint_hues(window: &mut Window, bounds: Bounds<Pixels>, share: f32) {
    let (left, top) = (bounds.origin.x.as_f32(), bounds.origin.y.as_f32());
    let wide = bounds.size.width.as_f32();
    let middle = top + bounds.size.height.as_f32() / 2.0;
    let bar = Bounds::new(
        point(px(left), px(middle - HUES_TRACK / 2.0)),
        size(px(wide), px(HUES_TRACK)),
    );
    // Six parts, a gradient each (GPUI's have two colours), in a mask with round ends.
    window.with_content_mask(Some(gpui::ContentMask { bounds: bar }), |window| {
        for part in 0..6 {
            let from = left + wide * part as f32 / 6.0;
            let to = left + wide * (part + 1) as f32 / 6.0;
            let piece = Bounds::new(
                point(px(from.floor()), bar.origin.y),
                size(px(to.ceil() - from.floor()), bar.size.height),
            );
            let corners = gpui::Corners {
                top_left: px(if part == 0 { HUES_TRACK / 2.0 } else { 0.0 }),
                bottom_left: px(if part == 0 { HUES_TRACK / 2.0 } else { 0.0 }),
                top_right: px(if part == 5 { HUES_TRACK / 2.0 } else { 0.0 }),
                bottom_right: px(if part == 5 { HUES_TRACK / 2.0 } else { 0.0 }),
            };
            window.paint_quad(
                fill(
                    piece,
                    linear_gradient(
                        90.0,
                        linear_color_stop(hue(part as f32 * 60.0), 0.0),
                        linear_color_stop(hue((part + 1) as f32 * 60.0), 1.0),
                    ),
                )
                .corner_radii(corners),
            );
        }
    });
    let thumb = left + HUES_THUMB / 2.0 + (wide - HUES_THUMB).max(0.0) * share;
    ring(window, (thumb, middle), HUES_THUMB);
}

/// A white ring with a dark edge: where a colour is, over colours.
pub(crate) fn ring(window: &mut Window, middle: (f32, f32), across: f32) {
    let edge = gpui::hsla(0.0, 0.0, 0.0, 0.6);
    let quad = |across: f32, width: f32, colour: Hsla| {
        fill(
            Bounds::new(
                point(px(middle.0 - across / 2.0), px(middle.1 - across / 2.0)),
                size(px(across), px(across)),
            ),
            gpui::transparent_black(),
        )
        .corner_radii(px(across / 2.0))
        .border_widths(px(width))
        .border_color(colour)
    };
    window.paint_quad(quad(across + 2.0, 1.0, edge));
    window.paint_quad(quad(across, 2.0, gpui::white()));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_value_is_where_the_middle_of_the_thumb_is_to_the_nearest_step() {
        // A track 120 across: the middle of the thumb goes from 10 to 110.
        let at = |x: f32| value_at(x, 0.0, 120.0, THUMB, (1.0, 20.0, 1.0));
        assert_eq!(at(0.0), 1.0);
        assert_eq!(at(10.0), 1.0);
        assert_eq!(at(110.0), 20.0);
        assert_eq!(at(500.0), 20.0);
        assert_eq!(at(60.0), 11.0);
        // In fives, from 5 to 100.
        let opacity = |x: f32| value_at(x, 0.0, 120.0, THUMB, (5.0, 100.0, 5.0));
        assert_eq!(opacity(10.0), 5.0);
        assert_eq!(opacity(60.0), 55.0);
        assert_eq!(opacity(110.0), 100.0);
        assert_eq!(share_of(55.0, 5.0, 100.0), 50.0 / 95.0);
    }

    #[test]
    fn the_hues_go_round() {
        assert_eq!(hue(0.0), gpui::hsla(0.0, 1.0, 0.5, 1.0));
        assert_eq!(hue(360.0), hue(0.0));
    }
}
