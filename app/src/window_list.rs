//! The list of windows of the window mode: each with its name above its picture, the way the
//! system's own task view lays them out (owner: 「Win + TAB みたいな見た目のほうがいいかな」「ウィンドウ名が
//! プレビューの上にあるの」). The one way to choose in that mode: nothing on the screen behind it
//! reacts. It also reaches windows hidden behind others.
//!
//! The pictures share one height, the largest at which every card still fits the monitor (「画面
//! まるまるいっぱい使いたい」); with more windows than fit even at the smallest, the list scrolls.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::f32::consts::PI;
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant};

use gpui::prelude::FluentBuilder;
use gpui::{
    AnyElement, Bounds, CursorStyle, Entity, Hsla, InteractiveElement, IntoElement, ObjectFit,
    ParentElement, PathBuilder, Pixels, RenderImage, StatefulInteractiveElement, Styled,
    StyledImage, Window, canvas, div, img, point, px, rgba,
};
use perora_core::language::Language;
use perora_core::platform::Window as SystemWindow;
use perora_core::selection::Choice;

use crate::selection::{self, Selection};

/// What a card adds around its picture, and what lies between cards (logical pixels): its padding
/// and border at the sides, the name and the gap above the picture, the padding and border below.
const SIDES: f32 = 24.0;
const ABOVE: f32 = 46.0;
const BELOW: f32 = 12.0;
const GAP: f32 = 20.0;
/// Where the list is on the monitor: under the bar, and off the edges.
const INSET_TOP: f32 = 116.0;
const INSET: f32 = 32.0;
/// The smallest the pictures get; with more windows than that leaves room for, the list scrolls.
const SMALLEST: f32 = 24.0;
/// No picture wider or taller than this many times its other side: a very wide window (a bar across
/// the monitor) or a very tall one would otherwise set the height for all.
const MOST_OBLONG: f32 = 2.4;
/// The largest a picture of the list is made, whatever room it has: beyond this it is drawn
/// enlarged (as in the Tauri version).
pub const THUMBNAIL_FIT: (u32, u32) = (1600, 1000);
/// How long a picture takes to fade in once it is there.
const FADE: Duration = Duration::from_millis(120);
/// How long the ring takes to turn once.
const TURN: Duration = Duration::from_millis(900);

/// The shape of a window's place in the list: its own, but never too oblong.
pub fn aspect(width: u32, height: u32) -> f32 {
    let ratio = width as f32 / height.max(1) as f32;
    ratio.clamp(1.0 / MOST_OBLONG, MOST_OBLONG)
}

/// The largest height the pictures can share so that the cards, wrapped into rows, fit the room;
/// and whether they fit at all (at the smallest height, when they do not: the list scrolls).
pub fn picture_height(aspects: &[f32], room_width: f32, room_height: f32) -> (f32, bool) {
    let fits = |height: f32| {
        let (mut rows, mut used) = (1u32, 0.0f32);
        for ratio in aspects {
            let card = height * ratio + SIDES;
            if card > room_width {
                return false;
            }
            if used > 0.0 && used + GAP + card > room_width {
                rows += 1;
                used = 0.0;
            }
            used += if used > 0.0 { GAP } else { 0.0 } + card;
        }
        rows as f32 * (height + ABOVE + BELOW) + (rows - 1) as f32 * GAP <= room_height
    };
    // Fitting gets no easier as the pictures grow: the largest height that fits is found by halving.
    let (mut low, mut high) = (SMALLEST, room_height.max(SMALLEST));
    while high - low > 1.0 {
        let middle = (low + high) / 2.0;
        if fits(middle) {
            low = middle;
        } else {
            high = middle;
        }
    }
    (low.floor(), fits(low.floor()))
}

/// The list as laid out on the monitor with the bar, and what it has of the pictures so far.
pub struct List {
    /// The height of every picture, and whether all the cards fit (logical pixels).
    pub height: f32,
    pub fits: bool,
    /// The small picture of each window (by its index), and when it came.
    pub pictures: HashMap<usize, (Arc<RenderImage>, Instant)>,
    /// No more pictures are coming: a place still without one stays empty.
    pub all_made: bool,
    /// Where the list and each card were last drawn: the system's pointer is the one shown over a
    /// card (owner: 「ウィンドウカード上はカーソルでほしいね」), the + between them.
    pub room: Rc<Cell<Option<Bounds<Pixels>>>>,
    pub cards: Rc<RefCell<Vec<Option<Bounds<Pixels>>>>>,
}

impl List {
    pub fn new(windows: &[SystemWindow], viewport_width: f32, viewport_height: f32) -> Self {
        let aspects: Vec<f32> = windows
            .iter()
            .map(|window| aspect(window.bounds.width, window.bounds.height))
            .collect();
        let (height, fits) = picture_height(
            &aspects,
            viewport_width - 2.0 * INSET,
            viewport_height - INSET_TOP - INSET,
        );
        Self {
            height,
            fits,
            pictures: HashMap::new(),
            all_made: false,
            room: Rc::default(),
            cards: Rc::new(RefCell::new(vec![None; windows.len()])),
        }
    }

    /// How large each window's small picture is made (physical pixels): the size of its place, at
    /// most `THUMBNAIL_FIT`. (Not larger than it is shown: there may be many.)
    pub fn sizes(&self, windows: &[SystemWindow], scale: f32) -> Vec<(u32, u32)> {
        windows
            .iter()
            .map(|window| {
                let ratio = aspect(window.bounds.width, window.bounds.height);
                let width = (self.height * ratio * scale).ceil() as u32;
                let height = (self.height * scale).ceil() as u32;
                (
                    width.clamp(1, THUMBNAIL_FIT.0),
                    height.clamp(1, THUMBNAIL_FIT.1),
                )
            })
            .collect()
    }

    /// Whether something is moving on it: a ring turning, a picture fading in.
    pub fn moving(&self, count: usize) -> bool {
        let waiting = !self.all_made && self.pictures.len() < count;
        waiting
            || self
                .pictures
                .values()
                .any(|(_, came)| came.elapsed() < FADE)
    }
}

pub fn element(
    list: &List,
    windows: &[SystemWindow],
    entity: &Entity<Selection>,
    language: &Language,
    accent: Hsla,
) -> AnyElement {
    if windows.is_empty() {
        return div()
            .absolute()
            .top(px(INSET_TOP))
            .left(px(INSET))
            .right(px(INSET))
            .bottom(px(INSET))
            .flex()
            .items_center()
            .justify_center()
            .text_color(gpui::white())
            .text_size(px(16.0))
            .child(language.text("overlay.windows.none", &[]))
            .into_any_element();
    }
    let untitled = language.text("overlay.windows.untitled", &[]);
    let height = px(list.height);
    let cards = windows.iter().enumerate().map(|(index, window)| {
        let width = height * aspect(window.bounds.width, window.bounds.height);
        let title = if window.title.is_empty() {
            untitled.clone()
        } else {
            window.title.clone()
        };
        let picture = list.pictures.get(&index).cloned();
        let waiting = picture.is_none() && !list.all_made;
        let chosen = entity.clone();
        let drawn = list.cards.clone();
        div()
            .id(("window", index))
            .relative()
            .flex()
            .flex_col()
            .gap(px(8.0))
            .px(px(10.0))
            .pb(px(10.0))
            .rounded(px(8.0))
            .border_2()
            .border_color(rgba(0x0000_0000))
            .bg(rgba(0x2020_20b8))
            .text_color(gpui::white())
            .cursor(CursorStyle::PointingHand)
            .hover(move |card| card.bg(rgba(0x3232_32e0)).border_color(accent))
            .on_click(move |_, _, cx| {
                let chosen = chosen.clone();
                cx.defer(move |cx| selection::finish(&chosen, Some(Choice::Window(index)), cx));
            })
            // As wide as the picture below it, never wider: a long name is cut, not the reason for
            // a wide card.
            .child(
                div()
                    .w(width)
                    .h(px(36.0))
                    .line_height(px(36.0))
                    .text_size(px(13.0))
                    .truncate()
                    .child(title),
            )
            .child(
                div()
                    .relative()
                    .w(width)
                    .h(height)
                    .rounded(px(4.0))
                    .bg(rgba(0x0000_0059))
                    .overflow_hidden()
                    .when_some(picture, |place, (picture, came)| {
                        let shown = (came.elapsed().as_secs_f32() / FADE.as_secs_f32()).min(1.0);
                        place.child(
                            img(picture)
                                .size_full()
                                .object_fit(ObjectFit::Contain)
                                .opacity(shown),
                        )
                    })
                    .when(waiting, |place| {
                        place.child(canvas(|_, _, _| {}, move |bounds, _, window, _| {
                            paint_ring(bounds, accent, window)
                        })
                        .absolute()
                        .size_full())
                    }),
            )
            .child(
                canvas(
                    move |bounds, _, _| {
                        if let Some(slot) = drawn.borrow_mut().get_mut(index) {
                            *slot = Some(bounds);
                        }
                    },
                    |_, _, _, _| {},
                )
                .absolute()
                .size_full(),
            )
            .into_any_element()
    });
    let room = list.room.clone();
    div()
        .absolute()
        .top(px(INSET_TOP))
        .left(px(INSET))
        .right(px(INSET))
        .bottom(px(INSET))
        // Where the list is seen: recorded out here, as what scrolls moves with the scrolling.
        .child(
            canvas(move |bounds, _, _| room.set(Some(bounds)), |_, _, _, _| {})
                .absolute()
                .size_full(),
        )
        .child(
            div()
                .id("windows")
                .size_full()
                .flex()
                .flex_wrap()
                .justify_center()
                // More windows than there is room for, even at the smallest: the rest is reached by
                // scrolling, from the first row on.
                .when(list.fits, |list| list.content_center())
                .when(!list.fits, |list| list.content_start())
                .gap(px(GAP))
                .overflow_y_scroll()
                .children(cards),
        )
        .into_any_element()
}

/// The ring of Windows that turns while something is on its way: 28 px, a faint circle and a
/// quarter of it in the accent colour.
fn paint_ring(bounds: Bounds<Pixels>, accent: Hsla, window: &mut Window) {
    const SIDE: f32 = 28.0;
    const WIDTH: f32 = 3.0;
    let centre = bounds.center();
    let radius = (SIDE - WIDTH) / 2.0;
    let turned = Instant::now()
        .duration_since(*STARTED)
        .as_secs_f32()
        .rem_euclid(TURN.as_secs_f32())
        / TURN.as_secs_f32();
    let at = |angle: f32| {
        point(
            centre.x + px(radius * angle.cos()),
            centre.y + px(radius * angle.sin()),
        )
    };
    let arc = |from: f32, to: f32, colour: Hsla, window: &mut Window| {
        let mut path = PathBuilder::stroke(px(WIDTH));
        path.move_to(at(from));
        path.arc_to(
            point(px(radius), px(radius)),
            px(0.0),
            to - from > PI,
            true,
            at(to),
        );
        if let Ok(path) = path.build() {
            window.paint_path(path, colour);
        }
    };
    arc(0.0, PI, gpui::white().opacity(0.18), window);
    arc(PI, 2.0 * PI, gpui::white().opacity(0.18), window);
    // The top quarter, turned.
    let middle = -PI / 2.0 + turned * 2.0 * PI;
    arc(middle - PI / 4.0, middle + PI / 4.0, accent, window);
}

static STARTED: std::sync::LazyLock<Instant> = std::sync::LazyLock::new(Instant::now);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_pictures_are_as_large_as_the_room_lets_every_card_be() {
        // One 16:9 window in a room of 1856 x 932: the height decides (932 - 46 - 12 = 874, found
        // to within a pixel).
        let (height, fits) = picture_height(&[16.0 / 9.0], 1856.0, 932.0);
        assert!(fits);
        assert!((873.0..=874.0).contains(&height), "{height}");
        // Three side by side do not fit in one row at that height: smaller, and still all there.
        let three = [16.0 / 9.0; 3];
        let (smaller, fits) = picture_height(&three, 1856.0, 932.0);
        assert!(fits);
        assert!(smaller < height);
        let one_more = picture_height(&three, 1856.0, 932.0).0 + 2.0;
        assert!(3.0 * (one_more * 16.0 / 9.0 + SIDES) + 2.0 * GAP > 1856.0 || 2.0 * (one_more + ABOVE + BELOW) + GAP > 932.0);
    }

    #[test]
    fn with_more_windows_than_room_the_list_scrolls_at_the_smallest() {
        let aspects = vec![16.0 / 9.0; 400];
        let (height, fits) = picture_height(&aspects, 1856.0, 932.0);
        assert_eq!(height, SMALLEST);
        assert!(!fits);
    }

    #[test]
    fn a_very_oblong_window_is_given_a_place_no_more_oblong_than_the_limit() {
        assert_eq!(aspect(3840, 40), MOST_OBLONG);
        assert_eq!(aspect(40, 2160), 1.0 / MOST_OBLONG);
        assert_eq!(aspect(1600, 900), 1600.0 / 900.0);
    }
}
