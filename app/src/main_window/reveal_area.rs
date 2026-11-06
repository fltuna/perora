//! The part of the monitor that brings the selection screen's mode bar back (`barRevealArea`): shown
//! on the capture page on a small 16:9 monitor with the bar where it is, and set on a screen of its
//! own over the whole monitor, as a capture is (owner, 2026-11-02: 「設定ボタン押したらキャプチャ画面
//! みたいなのが出てくるイメージ！」).

use gpui::{App, Hsla, IntoElement, ParentElement, Styled, div, px, rgb, rgba};
use perora_core::language::Language;
use perora_core::settings::Zone;
use perora_ui::{Button, Theme};

use super::system;

/// The small monitor, as wide as the style sheet has it, 16:9, and its edge.
const MONITOR: (f32, f32) = (240.0, 135.0);
const EDGE: f32 = 1.0;

/// The part, as the selection screen's tour and this page draw it: a see-through blue with an edge.
pub(crate) fn zone_fill() -> Hsla {
    rgba(0x4f8c_ff59).into()
}

pub(crate) fn zone_edge() -> Hsla {
    rgb(0x4f8cff).into()
}

/// The small monitor with the bar and the part, and the two buttons that set the part: on the screen
/// of its own, or back to where it is at first.
pub fn preview(zone: Zone, language: &Language, theme: &Theme) -> impl IntoElement {
    let t = |key: &str| language.text(key, &[]);
    // (Inside the edge: what the shares are of.)
    let (wide, high) = (MONITOR.0 - 2.0 * EDGE, MONITOR.1 - 2.0 * EDGE);
    let monitor = div()
        .relative()
        .flex_none()
        .w(px(MONITOR.0))
        .h(px(MONITOR.1))
        .bg(rgb(0x1b1e24))
        .border_1()
        .border_color(rgb(0x3a3f4a))
        .rounded(px(4.0))
        .overflow_hidden()
        // The bar, where the selection screen has it.
        .child(
            div()
                .absolute()
                .top(px(high * 0.03))
                .left(px(wide * (0.5 - 0.23)))
                .w(px(wide * 0.46))
                .h(px(high * 0.07))
                .bg(rgb(0x2a2e36))
                .border_1()
                .border_color(rgb(0x454b57))
                .rounded(px(3.0)),
        )
        // (At least a line, however thin: a strip of a few pixels at the top is less than one here.)
        .child(
            div()
                .absolute()
                .left(px(wide * zone.x as f32))
                .top(px(high * zone.y as f32))
                .w(px((wide * zone.width as f32).max(2.0)))
                .h(px((high * zone.height as f32).max(2.0)))
                .bg(zone_fill())
                .border_1()
                .border_color(zone_edge()),
        );
    let _ = theme;
    div()
        .flex_none()
        .flex()
        .flex_col()
        .items_end()
        .gap(px(8.0))
        .child(monitor)
        .child(
            div()
                .flex()
                .gap(px(8.0))
                .child(
                    Button::new("barRevealAreaEdit", t("main.capture.barRevealAreaEdit"))
                        .on_click(|_, cx| edit(cx)),
                )
                .child(
                    Button::new("barRevealAreaReset", t("main.capture.barRevealAreaReset"))
                        .on_click(|_, cx| {
                            system::set(cx, |settings| settings.bar_reveal_area = Zone::BAR_REVEAL)
                        }),
                ),
        )
}

/// The screen that sets the part, over the monitor the main window is on.
pub fn edit(cx: &mut App) {
    let _ = cx;
    eprintln!("the screen for the bar's area: not yet");
}
