//! The mode bar of the selection screen, on the monitor the pointer started on: the modes with the
//! key of each, giving up, and a hint for the mode.
//!
//! It slides up out of the way after a while (it can be in the way of what is captured: owner,
//! 2026-11-02), and comes back when the pointer goes into a part of the monitor drawn in the settings
//! (`barRevealArea`, at first a strip right at the top). Once back it stays while the pointer is at
//! its place, so that it can be used. A frame being dragged sends it away at once; a change of mode
//! brings it back for a while (by a key: the bar says which mode it is now).

use std::cell::Cell;
use std::rc::Rc;
use std::time::{Duration, Instant};

use gpui::prelude::FluentBuilder;
use gpui::{
    AnyElement, Bounds, BoxShadow, CursorStyle, Entity, FontWeight, InteractiveElement, IntoElement,
    ParentElement, Pixels, Size, StatefulInteractiveElement, Styled, canvas, div, point, px, rgba,
};
use perora_core::language::Language;
use perora_core::selection::Mode;
use perora_core::settings::Zone;

use crate::selection::{self, Selection};

/// How near the bar the pointer keeps it, once it is back (logical pixels).
const REACH: f32 = 24.0;
/// How far up from its place the bar goes when it slides away: its own height and this, past its
/// shadow (as `translate(-50%, calc(-100% - 24px - 32px))` did).
const AWAY: f32 = 24.0 + 32.0;
/// How long a slide takes.
const SLIDE: Duration = Duration::from_millis(200);
/// Where it is when shown.
const TOP: f32 = 24.0;

/// Where the bar was last drawn, in its window's logical pixels.
#[derive(Clone, Copy, Debug)]
pub struct Drawn {
    /// Where it is seen (slid up, or on the way).
    pub seen: Bounds<Pixels>,
    /// Its place, without the slide: what the pointer keeps it back by.
    pub place: Bounds<Pixels>,
}

pub struct Bar {
    /// After how long it slides away; 0 for never.
    seconds: f64,
    zone: Zone,
    near: bool,
    tucked: bool,
    /// Counted up by a change of mode: the bar comes back for a while.
    shown: u64,
    /// What it was last set for: whether it stays, whether a drag sent it away, and `shown`.
    set_for: Option<(bool, bool, u64)>,
    /// Counted up for every new wait: a wait that is over and is not the last is nothing.
    timer: u64,
    /// The slide: how far away it was when it began (0 at its place, 1 away), and when.
    slide_from: f32,
    slid_at: Option<Instant>,
    animates: bool,
    pub drawn: Rc<Cell<Option<Drawn>>>,
}

impl Bar {
    pub fn new(seconds: f64, zone: Zone) -> Self {
        Self {
            seconds,
            zone,
            near: false,
            tucked: false,
            shown: 0,
            set_for: None,
            timer: 0,
            slide_from: 0.0,
            slid_at: None,
            animates: perora_core::platform::animations_enabled(),
            drawn: Rc::default(),
        }
    }

    /// How far away it is now: 0 at its place, 1 away.
    fn away(&self) -> f32 {
        let target = if self.tucked { 1.0 } else { 0.0 };
        let Some(since) = self.slid_at.filter(|_| self.animates) else {
            return target;
        };
        let t = (since.elapsed().as_secs_f32() / SLIDE.as_secs_f32()).min(1.0);
        // Out of the way slowly at first, back quickly at first (ease-in and ease-out).
        let eased = if self.tucked {
            t * t
        } else {
            1.0 - (1.0 - t) * (1.0 - t)
        };
        self.slide_from + (target - self.slide_from) * eased
    }

    pub fn sliding(&self) -> bool {
        self.animates && self.slid_at.is_some_and(|since| since.elapsed() < SLIDE)
    }

    fn set_tucked(&mut self, tucked: bool) {
        if self.tucked != tucked {
            self.slide_from = self.away();
            self.slid_at = Some(Instant::now());
            self.tucked = tucked;
        }
    }

    /// Where the pointer is now, in the bar's window (`None`: off its monitor).
    pub fn pointer_at(&mut self, at: Option<gpui::Point<Pixels>>, viewport: Size<Pixels>) {
        let Some(at) = at else {
            self.near = false;
            return;
        };
        let zone = self.zone;
        let (x, y) = (
            f64::from(at.x / viewport.width),
            f64::from(at.y / viewport.height),
        );
        let in_zone = x >= zone.x && x <= zone.x + zone.width && y >= zone.y && y <= zone.y + zone.height;
        let at_place = self.drawn.get().is_some_and(|drawn| {
            let place = drawn.place;
            at.x >= place.left() - px(REACH)
                && at.x <= place.right() + px(REACH)
                && at.y <= place.bottom() + px(REACH)
        });
        // Slid away, only the part drawn in the settings brings it back; shown, its place keeps it.
        self.near = in_zone || (at_place && (self.near || !self.tucked));
    }

    /// A change of mode: back for a while.
    pub fn mode_changed(&mut self) {
        self.shown += 1;
    }

    /// Whether it is to stay or go, now: the wait to start, when one is (its number and how long).
    pub fn update(&mut self, dragging: bool) -> Option<(u64, f64)> {
        // (Not kept back by a frame being dragged up there: it would be in the way of that.)
        let pointed_at = self.near && !dragging;
        let stays = pointed_at || self.seconds <= 0.0;
        let dragged_away = dragging && self.seconds > 0.0;
        let set_for = (stays, dragged_away, self.shown);
        if self.set_for == Some(set_for) {
            return None;
        }
        self.set_for = Some(set_for);
        self.set_tucked(dragged_away);
        self.timer += 1;
        (!stays && !dragged_away).then_some((self.timer, self.seconds))
    }

    /// A wait is over: away, when it is the last one. Whether anything changed.
    pub fn waited(&mut self, timer: u64) -> bool {
        if timer != self.timer || self.tucked {
            return false;
        }
        self.set_tucked(true);
        true
    }
}

/// The bar, as drawn now: at its place, or on its way up or back.
pub fn element(
    selection: &Selection,
    entity: &Entity<Selection>,
    mode: Mode,
    language: &Language,
) -> AnyElement {
    let bar = selection.bar();
    let height = bar
        .drawn
        .get()
        .map_or(px(0.0), |drawn| drawn.place.size.height);
    let offset = (height + px(AWAY)) * bar.away();
    let top = px(TOP) - offset;
    let drawn = bar.drawn.clone();

    let modes = Mode::ALL.iter().enumerate().map(|(index, &each)| {
        let current = each == mode;
        let colour = if current { 0xffff_ffff } else { 0xeceef2ff };
        let entity = entity.clone();
        div()
            .id(("mode", index))
            .flex()
            .items_center()
            .gap(px(6.0))
            .h(px(32.0))
            .px(px(12.0))
            .rounded(px(6.0))
            .border_1()
            .border_color(rgba(0x0000_0000))
            .cursor(CursorStyle::PointingHand)
            .whitespace_nowrap()
            .text_color(rgba(colour))
            .when(current, |button| button.bg(rgba(CURRENT)))
            .when(!current, |button| button.hover(|button| button.bg(rgba(0x2326_2dff))))
            .on_click(move |_, _, cx| {
                let entity = entity.clone();
                cx.defer(move |cx| selection::set_mode(&entity, each, cx));
            })
            .child(key(index + 1, colour))
            .child(language.text(&format!("overlay.mode.{}", each.name()), &[]))
            .into_any_element()
    });
    let cancel = {
        let entity = entity.clone();
        div()
            .id("cancel")
            .flex()
            .items_center()
            .h(px(32.0))
            .px(px(12.0))
            .ml(px(6.0))
            .rounded(px(6.0))
            .border_1()
            .border_color(rgba(0x0000_0000))
            .cursor(CursorStyle::PointingHand)
            .whitespace_nowrap()
            .text_color(rgba(0x9aa1_adff))
            .hover(|button| button.bg(rgba(0x2326_2dff)))
            .on_click(move |_, _, cx| {
                let entity = entity.clone();
                cx.defer(move |cx| selection::finish(&entity, None, cx));
            })
            .child(language.text("overlay.cancel", &[]))
    };
    let hint_key = format!("overlay.hint.{}", mode.name());
    let hint = language.text(&hint_key, &[]);
    let hint = (hint != hint_key).then(|| {
        div()
            .px(px(10.0))
            .py(px(2.0))
            .rounded(px(999.0))
            .bg(rgba(0x0000_0099))
            .text_color(rgba(0xffff_ffff))
            .child(hint)
    });

    div()
        .absolute()
        .left_0()
        .top(top)
        .w_full()
        .flex()
        .justify_center()
        .child(
            div()
                .id("bar")
                .relative()
                .flex()
                .flex_col()
                .items_center()
                .gap(px(8.0))
                .cursor(CursorStyle::Arrow)
                .child(
                    div()
                        .flex()
                        .gap(px(4.0))
                        .p(px(6.0))
                        .rounded(px(10.0))
                        .border_1()
                        .border_color(rgba(0x353a_44ff))
                        .bg(rgba(0x1618_1deb))
                        .shadow(vec![BoxShadow {
                            color: rgba(0x0000_0073).into(),
                            offset: point(px(0.0), px(8.0)),
                            blur_radius: px(24.0),
                            spread_radius: px(0.0),
                            inset: false,
                        }])
                        .children(modes)
                        .child(cancel),
                )
                .children(hint)
                // Where it is, for the pointer to be told against (its place without the slide).
                .child(
                    canvas(
                        move |bounds, _, _| {
                            let mut place = bounds;
                            place.origin.y += offset;
                            drawn.set(Some(Drawn {
                                seen: bounds,
                                place,
                            }));
                        },
                        |_, _, _, _| {},
                    )
                    .absolute()
                    .size_full(),
                ),
        )
        .into_any_element()
}

/// The colour of the current mode (and of the frame around what would be chosen).
pub const CURRENT: u32 = 0x4f8c_ffff;

/// The key of a mode, in a small frame of the text's colour, a little faded.
fn key(number: usize, colour: u32) -> impl IntoElement {
    div()
        .min_w(px(18.0))
        .px(px(4.0))
        .rounded(px(4.0))
        .border_1()
        .border_color(rgba(colour))
        .opacity(0.7)
        .text_size(px(11.0))
        .font_weight(FontWeight::NORMAL)
        .flex()
        .justify_center()
        .child(number.to_string())
}
