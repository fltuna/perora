//! The other button of the mouse (owner: 「右クリック + ホイールでツール切り替え(+マウスカーソル右上
//! 付近にツールアイコンを表示)」): held, the wheel goes through the tools in the order of their bar
//! (stopping at its ends: 「ループはしなくていいかも」), the shapes and the two ways of hiding by the
//! last of them taken; the tool in hand is shown up and to the right of the pointer, going along
//! with it, and gone a moment after the button is let go (it comes up from below, and goes back
//! down: 「出るときは下からふわっと、いなくなるときは下に対してふわっとしてほしい」). A tool taken so
//! has its second bar put away.
//!
//! Pressed twice where the pointer is, over the picture, the settings of what is in hand (or
//! selected) are put there (「ダブル右クリックしたらその場にそのツールの設定出せたら利便性高そう！」):
//! its second bar 12 pixels under the pointer, or the panel of the pen or the marker. A press
//! elsewhere, Esc, another tool, another selection, and they are gone.

use std::time::{Duration, Instant};

use gpui::{
    Context, InteractiveElement, IntoElement, MouseButton, MouseDownEvent, ParentElement, Pixels,
    Point, ScrollDelta, ScrollEvent, Styled, Window, deferred, div, px,
};
use perora_core::platform;
use perora_ui::Theme;
use perora_ui::motion::{self, Curve};

use super::Editor;
use super::bars::Panel;
use crate::icons;
use crate::paint;
use crate::tools::Kind;

/// Two presses this close (in time, and on the screen) are pressed twice.
const TWICE_WITHIN: Duration = Duration::from_millis(450);
const TWICE_NEAR: f32 = 8.0;
/// How much of the wheel is a tool: a notch (three lines).
const A_NOTCH: f32 = 1.5;
/// How long the tool stays shown after the button is let go, and how it comes and goes.
const STAYS: Duration = Duration::from_millis(450);
const COMING: Duration = Duration::from_millis(200);
const COMES: Curve = Curve(0.1, 0.9, 0.2, 1.0);
const GOING: Duration = Duration::from_millis(160);
const GOES: Curve = Curve(0.7, 0.0, 0.84, 0.0);

#[derive(Default)]
pub(super) struct Turning {
    /// The other button is held.
    pub down: bool,
    /// How much the wheel has turned towards the next tool.
    by: f32,
    /// The tool shown by the pointer: where, since when, and since when it goes.
    shown: Option<(Point<Pixels>, Instant)>,
    let_go: Option<Instant>,
    /// The last press of the other button: when, and where.
    last: Option<(Instant, Point<Pixels>)>,
}

impl Editor {
    /// The tools in the order of their bar, the shapes and the ways of hiding by the last taken.
    fn round(&self) -> [Kind; 8] {
        [
            Kind::Select,
            Kind::Pen,
            Kind::Marker,
            self.figures.figure,
            Kind::Text,
            Kind::Stamp,
            self.hiders.kind,
            Kind::Crop,
        ]
    }

    /// The other button pressed: held (for the wheel); pressed twice over the picture, the settings
    /// there.
    pub(super) fn right_pressed(
        &mut self,
        event: &MouseDownEvent,
        over_picture: bool,
        cx: &mut Context<Self>,
    ) {
        self.turning.down = true;
        let now = Instant::now();
        let twice = self.turning.last.is_some_and(|(at, place)| {
            now.duration_since(at) < TWICE_WITHIN
                && (place.x - event.position.x)
                    .as_f32()
                    .hypot((place.y - event.position.y).as_f32())
                    < TWICE_NEAR
        });
        self.turning.last = if twice {
            None
        } else {
            Some((now, event.position))
        };
        if !twice || !over_picture || self.frozen || self.gesture.is_some() || self.typing.is_some()
        {
            return;
        }
        let stroke = self.tool.is_stroke() && self.chosen().is_none();
        if self.offered().is_some() || stroke {
            self.afloat = Some(event.position);
            self.asked = stroke.then(|| Panel::of_stroke(self.tool));
            cx.notify();
        }
    }

    /// The other button let go (or found let go): the tool shown goes in a moment.
    pub(super) fn right_let_go(&mut self, cx: &mut Context<Self>) {
        if self.turning.down {
            self.turning.down = false;
            self.turning.by = 0.0;
            self.turning.let_go = Some(Instant::now());
            cx.notify();
        }
    }

    /// The pointer moved: the tool shown goes along.
    pub(super) fn turning_moved(
        &mut self,
        position: Point<Pixels>,
        pressed: Option<MouseButton>,
        cx: &mut Context<Self>,
    ) {
        if self.turning.down && pressed != Some(MouseButton::Right) {
            self.right_let_go(cx);
        }
        if let Some((at, _)) = self.turning.shown.as_mut() {
            *at = position;
            cx.notify();
        }
    }

    /// The wheel with the other button held: the next tool or the one before. Whether it was that.
    pub(super) fn turned(&mut self, event: &ScrollEvent, cx: &mut Context<Self>) -> bool {
        if !self.turning.down {
            return false;
        }
        let now = Instant::now();
        if self.turning.shown.is_none() || self.turning.let_go.is_some() {
            self.turning.shown = Some((event.position, now));
        } else if let Some((at, _)) = self.turning.shown.as_mut() {
            *at = event.position;
        }
        self.turning.let_go = None;
        cx.notify();
        if self.frozen || self.gesture.is_some() {
            return true;
        }
        let lines = match event.delta {
            ScrollDelta::Lines(lines) => lines.y,
            ScrollDelta::Pixels(pixels) => pixels.y.as_f32() * 3.0 / 100.0,
        };
        self.turning.by += lines;
        if self.turning.by.abs() < A_NOTCH {
            return true;
        }
        // (Turned down: the next; up: the one before.)
        let step: isize = if self.turning.by < 0.0 { 1 } else { -1 };
        self.turning.by = 0.0;
        // (A press of that button right after this is the wheel again, not one of two presses.)
        self.turning.last = None;
        let round = self.round();
        let held = if self.tool.is_figure() {
            self.figures.figure
        } else if self.tool.is_hider() {
            self.hiders.kind
        } else {
            self.tool
        };
        let at = round.iter().position(|kind| *kind == held).unwrap_or(0) as isize;
        let Some(next) = usize::try_from(at + step).ok().and_then(|at| round.get(at)) else {
            return true;
        };
        if *next != self.tool {
            self.by_button = None;
            self.afloat = None;
            // (The tour's turns are not the user's choice of tool.)
            if self.touring.is_some() {
                self.tour_hold(*next, cx);
            } else {
                self.hold(*next, cx);
            }
        }
        true
    }

    /// The tool in hand by the pointer, while it is shown.
    pub(super) fn turned_view(
        &mut self,
        theme: &Theme,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<impl IntoElement + use<>> {
        let (at, since) = self.turning.shown?;
        let moving = platform::animations_enabled();
        let share = match self.turning.let_go {
            Some(let_go) => {
                let after = let_go.elapsed();
                if after >= STAYS + GOING || (!moving && after >= STAYS) {
                    self.turning.shown = None;
                    self.turning.let_go = None;
                    return None;
                }
                window.request_animation_frame();
                if after < STAYS {
                    1.0
                } else {
                    1.0 - GOES.at(motion::share(let_go, STAYS, GOING))
                }
            }
            None if moving => COMES.at(motion::along(since, Duration::ZERO, COMING, window)),
            None => 1.0,
        };
        let _ = cx;
        let colour = match self.tool {
            Kind::Pen => Some(paint::colour(self.pen.colour)),
            Kind::Marker => Some(paint::colour(self.marker.colour)),
            Kind::Stamp => Some(paint::colour(self.stamps.colour)),
            Kind::Text => Some(paint::colour(self.letters.color)),
            kind if kind.is_figure() => Some(paint::colour(self.figures.colour)),
            _ => None,
        };
        let inside = (self.tool.is_figure() && self.figures.fill > 0.0)
            .then(|| (paint::colour(self.figures.fill_colour), self.figures.fill));
        Some(
            deferred(
                div()
                    .absolute()
                    .left(at.x + px(14.0))
                    .top(at.y - px(38.0) + px(10.0 * (1.0 - share)))
                    .opacity(share)
                    .size(px(30.0))
                    .flex()
                    .items_center()
                    .justify_center()
                    .bg(theme.surface)
                    .border_1()
                    .border_color(theme.accent)
                    .rounded(theme.radius)
                    .shadow(super::bars::shadow(4.0, 14.0, 0x0000_004d))
                    .child(icons::icon(
                        icons::of_tool(self.tool, colour, inside),
                        16.0,
                        theme.text,
                    )),
            )
            .with_priority(3),
        )
    }
}

/// Where the settings asked for by the pointer go: the bar under it, in its middle.
pub(super) fn afloat_bar(at: Point<Pixels>, bar: impl IntoElement) -> impl IntoElement {
    // (In the middle of a row as wide as the window around the pointer.)
    div()
        .id("afloat")
        .absolute()
        .left(at.x - px(2000.0))
        .w(px(4000.0))
        .top(at.y + px(12.0))
        .flex()
        .justify_center()
        .child(bar)
}
