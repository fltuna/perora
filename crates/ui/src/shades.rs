//! Every shade of one hue, to pick one from: paler to the left, darker to the bottom (`.shades` in
//! the Tauri version's style.css), with a ring where the colour is. Pressed and dragged, it says the
//! saturation (across) and the value (up) the pointer is at.

use std::rc::Rc;

use gpui::{
    App, Bounds, CursorStyle, ElementId, Hsla, IntoElement, RenderOnce, Styled, Window, canvas,
    fill, linear_color_stop, linear_gradient, point, px, size,
};

use crate::Theme;
use crate::slider::{Follow, follow, hue, ring};

type OnChange = Rc<dyn Fn(f32, f32, &mut Window, &mut App)>;

#[derive(IntoElement)]
pub struct Shades {
    id: ElementId,
    hue: f32,
    saturation: f32,
    value: f32,
    colour: Hsla,
    on_change: Option<OnChange>,
}

impl Shades {
    /// The shades of `hue` (0 to 360), with the ring at `saturation` and `value` (0 to 1), in
    /// `colour` (the colour itself).
    pub fn new(
        id: impl Into<ElementId>,
        hue: f32,
        saturation: f32,
        value: f32,
        colour: Hsla,
    ) -> Self {
        Self {
            id: id.into(),
            hue,
            saturation,
            value,
            colour,
            on_change: None,
        }
    }

    /// Told of the saturation and value where the pointer is, pressed and dragged.
    pub fn on_change(
        mut self,
        on_change: impl Fn(f32, f32, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_change = Some(Rc::new(on_change));
        self
    }
}

impl RenderOnce for Shades {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let radius = Theme::get(cx).radius_small;
        let held = window.use_keyed_state(self.id.clone(), cx, |_, _| false);
        let Shades {
            hue: degrees,
            saturation,
            value,
            colour,
            on_change,
            ..
        } = self;
        canvas(
            |bounds, window, _| window.insert_hitbox(bounds, gpui::HitboxBehavior::Normal),
            move |bounds, hitbox, window, _| {
                window.paint_quad(
                    fill(
                        bounds,
                        linear_gradient(
                            90.0,
                            linear_color_stop(gpui::white(), 0.0),
                            linear_color_stop(hue(degrees), 1.0),
                        ),
                    )
                    .corner_radii(radius),
                );
                window.paint_quad(
                    fill(
                        bounds,
                        linear_gradient(
                            0.0,
                            linear_color_stop(gpui::black(), 0.0),
                            linear_color_stop(gpui::transparent_black(), 1.0),
                        ),
                    )
                    .corner_radii(radius),
                );
                let at = (
                    bounds.origin.x.as_f32() + bounds.size.width.as_f32() * saturation,
                    bounds.origin.y.as_f32() + bounds.size.height.as_f32() * (1.0 - value),
                );
                window.paint_quad(
                    fill(
                        Bounds::new(
                            point(px(at.0 - 4.0), px(at.1 - 4.0)),
                            size(px(8.0), px(8.0)),
                        ),
                        colour,
                    )
                    .corner_radii(px(4.0)),
                );
                ring(window, at, 12.0);
                window.set_cursor_style(CursorStyle::Crosshair, &hitbox);
                let Some(on_change) = on_change else {
                    return;
                };
                let to: Follow = Rc::new(move |position, window, cx| {
                    let across = ((position.x - bounds.origin.x).as_f32()
                        / bounds.size.width.as_f32().max(1.0))
                    .clamp(0.0, 1.0);
                    let down = ((position.y - bounds.origin.y).as_f32()
                        / bounds.size.height.as_f32().max(1.0))
                    .clamp(0.0, 1.0);
                    on_change(across, 1.0 - down, window, cx);
                });
                follow(window, hitbox, held, to);
            },
        )
        .w_full()
        .flex_1()
        .min_h(px(60.0))
    }
}
