//! A slider: a value between two ends, in steps, set by pressing on it and dragging (the Tauri
//! version's `<input type="range">`), in the look of Windows 11: a thin track, filled in the accent
//! colour up to the value, and a round thumb with the accent in its middle.
//!
//! The pointer is followed while the button is held, also beside the slider and outside the window:
//! every move of the window is heard (GPUI has no capture of the pointer), and only the one that
//! began on the slider is taken.

use std::rc::Rc;

use gpui::{
    App, Bounds, DispatchPhase, ElementId, Hsla, IntoElement, MouseButton, MouseDownEvent,
    MouseMoveEvent, MouseUpEvent, Pixels, Point, RenderOnce, Styled, Window, canvas, fill, point,
    px, size,
};

use crate::Theme;

/// How wide the thumb is, and the track.
const THUMB: f32 = 20.0;
const TRACK: f32 = 4.0;

#[derive(IntoElement)]
pub struct Slider {
    id: ElementId,
    value: f32,
    least: f32,
    most: f32,
    step: f32,
    enabled: bool,
    on_change: Option<OnChange>,
}

type OnChange = Rc<dyn Fn(f32, &mut Window, &mut App)>;

impl Slider {
    pub fn new(id: impl Into<ElementId>, value: f32, least: f32, most: f32) -> Self {
        Self {
            id: id.into(),
            value,
            least,
            most,
            step: 1.0,
            enabled: true,
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

    /// Told of each value the slider is moved to (not again of the one it is at).
    pub fn on_change(mut self, on_change: impl Fn(f32, &mut Window, &mut App) + 'static) -> Self {
        self.on_change = Some(Rc::new(on_change));
        self
    }
}

/// The value at `x` across a track from `left`, `wide` across (logical pixels), to the nearest step.
fn value_at(x: f32, left: f32, wide: f32, least: f32, most: f32, step: f32) -> f32 {
    // (The ends of the track are where the middle of the thumb can go.)
    let room = (wide - THUMB).max(1.0);
    let share = ((x - left - THUMB / 2.0) / room).clamp(0.0, 1.0);
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

impl RenderOnce for Slider {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = Theme::get(cx).clone();
        // The value last given while the thumb is held (none while it is not).
        let held = window.use_keyed_state(self.id.clone(), cx, |_, _| None::<f32>);
        let Slider {
            value,
            least,
            most,
            step,
            enabled,
            on_change,
            ..
        } = self;
        canvas(
            |bounds, window, _| window.insert_hitbox(bounds, gpui::HitboxBehavior::Normal),
            move |bounds, hitbox, window, _| {
                paint(
                    window,
                    bounds,
                    share_of(value, least, most),
                    enabled,
                    &theme,
                );
                if !enabled {
                    return;
                }
                let (left, wide) = (bounds.origin.x.as_f32(), bounds.size.width.as_f32());
                let set = {
                    let held = held.clone();
                    move |position: Point<Pixels>, window: &mut Window, cx: &mut App| {
                        let to = value_at(position.x.as_f32(), left, wide, least, most, step);
                        let last = held.read(cx).unwrap_or(value);
                        held.update(cx, |held, _| *held = Some(to));
                        if to != last
                            && let Some(on_change) = &on_change
                        {
                            on_change(to, window, cx);
                        }
                    }
                };
                let down = set.clone();
                window.on_mouse_event(move |event: &MouseDownEvent, phase, window, cx| {
                    if phase == DispatchPhase::Bubble
                        && event.button == MouseButton::Left
                        && hitbox.is_hovered(window)
                    {
                        cx.stop_propagation();
                        down(event.position, window, cx);
                    }
                });
                let moved = held.clone();
                window.on_mouse_event(move |event: &MouseMoveEvent, phase, window, cx| {
                    if phase != DispatchPhase::Bubble || moved.read(cx).is_none() {
                        return;
                    }
                    if event.pressed_button == Some(MouseButton::Left) {
                        set(event.position, window, cx);
                    } else {
                        // (Let go where the window did not hear it.)
                        moved.update(cx, |held, _| *held = None);
                    }
                });
                window.on_mouse_event(move |event: &MouseUpEvent, phase, _, cx| {
                    if phase == DispatchPhase::Bubble
                        && event.button == MouseButton::Left
                        && held.read(cx).is_some()
                    {
                        held.update(cx, |held, _| *held = None);
                    }
                });
            },
        )
        .h(px(THUMB))
        .w_full()
        .min_w(px(THUMB * 2.0))
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
    let disc = |across: f32, colour: Hsla, border: Option<Hsla>, window: &mut Window| {
        let mut quad = fill(
            Bounds::new(
                point(px(thumb - across / 2.0), px(middle - across / 2.0)),
                size(px(across), px(across)),
            ),
            colour,
        )
        .corner_radii(px(across / 2.0));
        if let Some(border) = border {
            quad = quad.border_widths(px(1.0)).border_color(border);
        }
        window.paint_quad(quad);
    };
    disc(THUMB, theme.surface_hover, Some(theme.border), window);
    disc(12.0, faded(theme.accent), None, window);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_value_is_where_the_middle_of_the_thumb_is_to_the_nearest_step() {
        // A track 120 across: the middle of the thumb goes from 10 to 110.
        let at = |x: f32| value_at(x, 0.0, 120.0, 1.0, 20.0, 1.0);
        assert_eq!(at(0.0), 1.0);
        assert_eq!(at(10.0), 1.0);
        assert_eq!(at(110.0), 20.0);
        assert_eq!(at(500.0), 20.0);
        assert_eq!(at(60.0), 11.0);
        // In fives, from 5 to 100.
        let opacity = |x: f32| value_at(x, 0.0, 120.0, 5.0, 100.0, 5.0);
        assert_eq!(opacity(10.0), 5.0);
        assert_eq!(opacity(60.0), 55.0);
        assert_eq!(opacity(110.0), 100.0);
        assert_eq!(share_of(55.0, 5.0, 100.0), 50.0 / 95.0);
    }
}
