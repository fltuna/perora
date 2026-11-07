//! A tour: what one of the windows shows of itself, a step at a time. The window is dimmed but for
//! what a step is about, and beside that is what there is to say of it. It comes once on its own, the
//! first time the window is seen, and again whenever its button (an "i") is pressed. Seen to the end
//! or put away, it does not come on its own again; a window that goes out of sight while it is on
//! leaves it unseen. (The window it is in sees to that: here is what is drawn, and the keys.)

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::time::{Duration, Instant};

use gpui::prelude::FluentBuilder;
use gpui::{
    App, BorderStyle, Bounds, Corners, ElementId, Hsla, InteractiveElement, IntoElement,
    KeyDownEvent, MouseButton, ParentElement, PathBuilder, Pixels, Point, SharedString,
    StatefulInteractiveElement, Styled, Window, canvas, div, point, px, quad, size,
};

use crate::Theme;
use crate::motion::{self, EASE, EASE_IN_OUT};

/// Around what a step is about; between that and what is said of it; between what is said and the
/// edge of the window.
const AROUND: f32 = 6.0;
const GAP: f32 = 10.0;
const EDGE: f32 = 8.0;
/// How long the frame and what is said take to go to the next step, and the words to come.
const MOVES: Duration = Duration::from_millis(250);
const FADES: Duration = Duration::from_millis(160);
/// The frame being dragged open, again and again.
const DRAGGED: Duration = Duration::from_millis(2600);
/// What a step is about.
#[derive(Debug, Clone, PartialEq)]
pub enum At {
    /// Nothing in particular: said in the middle of the window, all of it dimmed.
    Nothing,
    /// Something in the window, by the name it was marked with (`Marks::mark`).
    Mark(&'static str),
    /// A place in the window, to the pixel (a window on the screen, a frame being dragged).
    Place(Bounds<Pixels>),
}

/// How what a step is about is shown.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Look {
    /// A frame, the rest dimmed.
    #[default]
    Frame,
    /// A frame being dragged open from its corner, again and again.
    Drag,
    /// A ring alone, nothing dimmed: for what is shown on the rest of the window meanwhile.
    Ring,
}

#[derive(Debug, Clone)]
pub struct Step {
    pub at: At,
    pub text: SharedString,
    pub look: Look,
}

impl Step {
    pub fn new(at: At, text: impl Into<SharedString>) -> Self {
        Self {
            at,
            text: text.into(),
            look: Look::Frame,
        }
    }

    pub fn look(mut self, look: Look) -> Self {
        self.look = look;
        self
    }
}

/// What the tour asks of the window it is in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Asked {
    /// Shown, as far as a step: the window may have something to do for it (it is the tour's
    /// `at()` now).
    Moved,
    /// Over: seen to the end, or put away.
    End,
}

/// What a key is to the tour.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Keyed {
    /// It did something, or nothing at all: in either case it is the tour's, and nothing else is to
    /// hear it.
    Taken(Option<Asked>),
    /// Not the tour's (Alt and what goes with it: the system's).
    NotMine,
}

/// Where the things a step can be about are in the window. Marked as they are laid out, frame by
/// frame; what a tour reads is the frame before (whole by then). The window turns it at the start of
/// each of its frames (`turn`).
#[derive(Clone, Default)]
pub struct Marks(Rc<RefCell<Marked>>);

#[derive(Default)]
struct Marked {
    now: HashMap<&'static str, Bounds<Pixels>>,
    before: HashMap<&'static str, Bounds<Pixels>>,
}

impl Marks {
    /// Put inside what it marks (which is to be `relative`): where that is, from frame to frame.
    pub fn mark(&self, name: &'static str) -> impl IntoElement + use<> {
        let marks = self.0.clone();
        canvas(
            move |bounds, _, _| {
                marks.borrow_mut().now.insert(name, bounds);
            },
            |_, _, _, _| {},
        )
        .absolute()
        .top_0()
        .left_0()
        .size_full()
    }

    /// A new frame of the window: what was marked in the last is what the tour goes by.
    pub fn turn(&self) {
        let mut marked = self.0.borrow_mut();
        marked.before = std::mem::take(&mut marked.now);
    }

    /// Where that is, as the last frame had it.
    pub fn get(&self, name: &str) -> Option<Bounds<Pixels>> {
        let marked = self.0.borrow();
        marked
            .before
            .get(name)
            .or_else(|| marked.now.get(name))
            .copied()
    }
}

/// The words of the tour's own buttons, in the user's language.
pub struct Words {
    /// "{at} / {of}", the two filled in.
    pub count: Box<dyn Fn(usize, usize) -> String>,
    pub skip: SharedString,
    pub back: SharedString,
    pub next: SharedString,
    pub done: SharedString,
}

/// Something that goes from where it was to where it is to be over `MOVES`.
#[derive(Debug, Clone, Copy)]
struct Moving {
    from: Bounds<Pixels>,
    to: Bounds<Pixels>,
    began: Instant,
}

impl Moving {
    fn new(at: Bounds<Pixels>) -> Self {
        Self {
            from: at,
            to: at,
            began: Instant::now() - MOVES,
        }
    }

    /// Where it is now, asked to go `to` (from where it is, when that is somewhere else).
    fn toward(&mut self, to: Bounds<Pixels>, moves: bool, window: &mut Window) -> Bounds<Pixels> {
        if to != self.to {
            self.from = if moves { self.now() } else { to };
            self.to = to;
            self.began = Instant::now();
        }
        if !moves {
            return to;
        }
        let along = motion::along(self.began, Duration::ZERO, MOVES, window);
        between(self.from, self.to, EASE.at(along))
    }

    fn now(&self) -> Bounds<Pixels> {
        between(
            self.from,
            self.to,
            EASE.at(motion::share(self.began, Duration::ZERO, MOVES)),
        )
    }
}

fn between(from: Bounds<Pixels>, to: Bounds<Pixels>, along: f32) -> Bounds<Pixels> {
    let at = |a: Pixels, b: Pixels| a + (b - a) * along;
    Bounds::new(
        point(
            at(from.origin.x, to.origin.x),
            at(from.origin.y, to.origin.y),
        ),
        size(
            at(from.size.width, to.size.width),
            at(from.size.height, to.size.height),
        ),
    )
}

pub struct Tour {
    steps: Vec<Step>,
    at: usize,
    began: Instant,
    step_began: Instant,
    /// Whether things move (the system's animations on).
    motion: bool,
    /// The small preview's: smaller words and buttons.
    small: bool,
    spot: Option<Moving>,
    saying: Option<Moving>,
    /// How large what is said was, as last laid out.
    said: Rc<RefCell<Option<gpui::Size<Pixels>>>>,
    /// Which of the tour's buttons the keyboard is on (Tab goes from one to the next).
    focused: Option<usize>,
    /// The selection screen's look: the frame square and blue, the rest a little less dark.
    square: bool,
}

impl Tour {
    pub fn new(steps: Vec<Step>, motion: bool, small: bool) -> Self {
        let now = Instant::now();
        Self {
            steps,
            at: 0,
            began: now,
            step_began: now,
            motion,
            small,
            spot: None,
            saying: None,
            said: Rc::default(),
            focused: None,
            square: false,
        }
    }

    /// In the look of the selection screen.
    pub fn square(mut self) -> Self {
        self.square = true;
        self
    }

    /// Which step it is on.
    pub fn at(&self) -> usize {
        self.at
    }

    pub fn step(&self) -> &Step {
        &self.steps[self.at.min(self.steps.len().saturating_sub(1))]
    }

    /// The steps anew (what they say, where they are about), on the same step as far as there is one.
    pub fn set_steps(&mut self, steps: Vec<Step>) {
        self.steps = steps;
        self.at = self.at.min(self.steps.len().saturating_sub(1));
    }

    /// When the step it is on began.
    pub fn step_began(&self) -> Instant {
        self.step_began
    }

    fn last(&self) -> bool {
        self.at + 1 >= self.steps.len()
    }

    pub fn next(&mut self) -> Asked {
        if self.last() {
            return Asked::End;
        }
        self.go(self.at + 1)
    }

    pub fn back(&mut self) -> Asked {
        self.go(self.at.saturating_sub(1))
    }

    fn go(&mut self, to: usize) -> Asked {
        if to != self.at {
            self.at = to;
            self.step_began = Instant::now();
        }
        Asked::Moved
    }

    /// The keys are the tour's while it is on: → or Enter for the next step, ← for the one before,
    /// Esc to put it away. No other key does anything meanwhile (Tab goes from button to button of
    /// the tour, and Alt is the system's). A key held down does it once.
    pub fn key(&mut self, event: &KeyDownEvent) -> Keyed {
        let keystroke = &event.keystroke;
        if keystroke.modifiers.alt {
            return Keyed::NotMine;
        }
        if event.is_held {
            return Keyed::Taken(None);
        }
        let asked = match keystroke.key.as_str() {
            "escape" => Some(Asked::End),
            "right" | "enter" => Some(self.next()),
            "left" => Some(self.back()),
            "tab" => {
                let count = self.buttons().len();
                let from = self.focused.unwrap_or(count - 1);
                self.focused = Some(if keystroke.modifiers.shift {
                    (from + count - 1) % count
                } else {
                    (from + 1) % count
                });
                None
            }
            _ => None,
        };
        Keyed::Taken(asked)
    }

    /// The tour's buttons, as they are now: skip (not on the last step), back (after the first), and
    /// next or done.
    fn buttons(&self) -> Vec<Button> {
        let mut buttons = Vec::new();
        if !self.last() {
            buttons.push(Button::Skip);
        }
        if self.at > 0 {
            buttons.push(Button::Back);
        }
        buttons.push(Button::Next);
        buttons
    }

    /// Where the step is about, in the window, with room around what was marked.
    fn place(&self, marks: &Marks) -> Option<Bounds<Pixels>> {
        match &self.step().at {
            At::Nothing => None,
            At::Place(place) => Some(*place),
            At::Mark(name) => {
                let bounds = marks.get(name)?;
                if bounds.size.width <= px(0.0) && bounds.size.height <= px(0.0) {
                    return None;
                }
                Some(bounds.dilate(px(AROUND)))
            }
        }
    }

    /// The tour over the whole window: the dimming with its frame, and what is said with its
    /// buttons. Nothing pressed on it reaches what is under it. `asked` hears what its buttons ask
    /// (the window has the tour then: it calls `next`, `back`, or ends it).
    pub fn view(
        &mut self,
        marks: &Marks,
        words: &Words,
        theme: &Theme,
        window: &mut Window,
        asked: impl Fn(Button, &mut Window, &mut App) + 'static,
    ) -> gpui::AnyElement {
        let room = window.viewport_size();
        let whole = Bounds::new(point(px(0.0), px(0.0)), room);
        let step = self.step().clone();
        let place = self.place(marks);
        let moves = self.motion;
        // The frame: from where it was to where it is now (from the middle, coming from nothing).
        let spot_to = place.unwrap_or_else(|| Bounds::new(whole.center(), size(px(0.0), px(0.0))));
        let spot = match self.spot.as_mut() {
            Some(spot) => spot.toward(spot_to, moves && step.look != Look::Drag, window),
            None => {
                self.spot = Some(Moving::new(spot_to));
                spot_to
            }
        };
        // What is said: where it fits beside the frame, once its size is known.
        let measured = *self.said.borrow();
        let saying = measured.map(|measured| {
            let to = Bounds::new(saying_at(place, measured, room), measured);
            match self.saying.as_mut() {
                Some(saying) => saying.toward(to, moves, window).origin,
                None => {
                    self.saying = Some(Moving::new(to));
                    to.origin
                }
            }
        });
        if measured.is_none() {
            window.request_animation_frame();
        }
        let fade = |began: Instant, window: &mut Window| {
            if moves {
                motion::along(began, Duration::ZERO, FADES, window)
            } else {
                1.0
            }
        };
        let layer = fade(self.began, window);
        let words_in = fade(self.step_began, window);
        let dragging = (step.look == Look::Drag && moves).then(|| {
            window.request_animation_frame();
            let cycle = (self.step_began.elapsed().as_secs_f32() / DRAGGED.as_secs_f32()).fract();
            EASE_IN_OUT.at(((cycle - 0.12) / 0.5).clamp(0.0, 1.0))
        });
        let look = if place.is_none() {
            None
        } else {
            Some(step.look)
        };
        // (The selection screen's tour: on a screen that is always dark.)
        let accent = if self.square {
            theme.accent_dark
        } else {
            theme.accent
        };
        let square = self.square;

        let measure = {
            let said = self.said.clone();
            canvas(
                move |bounds, window, _| {
                    let mut said = said.borrow_mut();
                    if *said != Some(bounds.size) {
                        *said = Some(bounds.size);
                        window.request_animation_frame();
                    }
                },
                |_, _, _, _| {},
            )
            .absolute()
            .top_0()
            .left_0()
            .size_full()
        };
        let small = self.small;
        let focused = self.focused;
        let asked = Rc::new(asked);
        let button = |which: Button, index: usize, label: SharedString| {
            let asked = asked.clone();
            let primary = which == Button::Next;
            let quiet = which == Button::Skip;
            div()
                .id(ElementId::Name(format!("tour-{which:?}").into()))
                .h(px(if small { 24.0 } else { 28.0 }))
                .px(px(if small { 8.0 } else { 12.0 }))
                .flex()
                .items_center()
                .rounded(px(6.0))
                .border_1()
                .whitespace_nowrap()
                .map(|button| {
                    if primary {
                        button
                            .bg(theme.accent)
                            .border_color(theme.accent)
                            .text_color(theme.accent_text)
                    } else if quiet {
                        let text = theme.text;
                        button
                            .text_color(theme.muted)
                            .border_color(gpui::transparent_black())
                            .hover(move |style| style.text_color(text))
                    } else {
                        button
                            .bg(theme.surface_hover)
                            .border_color(theme.border)
                            .text_color(theme.text)
                    }
                })
                .when(focused == Some(index), |button| {
                    button.border_color(theme.text)
                })
                .on_mouse_down(MouseButton::Left, |_, window, cx| {
                    window.prevent_default();
                    cx.stop_propagation();
                })
                .on_click(move |_, window, cx| {
                    cx.stop_propagation();
                    asked(which, window, cx);
                })
                .child(label)
        };
        let buttons = self.buttons();
        let foot = div()
            .flex()
            .items_center()
            .gap(px(6.0))
            .child(
                div()
                    .mr_auto()
                    .pr(px(8.0))
                    .text_size(px(12.0))
                    .text_color(theme.muted)
                    .child((words.count)(self.at + 1, self.steps.len())),
            )
            .children(buttons.iter().enumerate().map(|(index, &which)| {
                let label = match which {
                    Button::Skip => words.skip.clone(),
                    Button::Back => words.back.clone(),
                    Button::Next if self.last() => words.done.clone(),
                    Button::Next => words.next.clone(),
                };
                button(which, index, label)
            }));
        let card = div()
            .absolute()
            .map(|card| match saying {
                Some(at) => card.left(at.x).top(at.y),
                None => card.left(px(0.0)).top(px(0.0)).opacity(0.0),
            })
            .max_w(px(320.0).min(room.width - px(16.0)))
            .when(small, |card| card.px(px(10.0)).pt(px(8.0)).pb(px(8.0)))
            .when(!small, |card| card.px(px(14.0)).pt(px(12.0)).pb(px(10.0)))
            .text_size(px(if small { 12.0 } else { 13.0 }))
            .line_height(gpui::relative(1.5))
            .text_color(theme.text)
            .bg(theme.surface)
            .border_1()
            .border_color(theme.border)
            .rounded(px(8.0))
            .shadow(vec![gpui::BoxShadow {
                color: gpui::hsla(0.0, 0.0, 0.0, 0.35),
                offset: point(px(0.0), px(8.0)),
                blur_radius: px(24.0),
                spread_radius: px(0.0),
                inset: false,
            }])
            .child(
                div()
                    .mb(px(if small { 6.0 } else { 10.0 }))
                    .opacity(words_in)
                    .child(step.text.clone()),
            )
            .child(foot)
            .child(measure);

        div()
            .id("tour")
            .absolute()
            .inset_0()
            .opacity(layer)
            .occlude()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_mouse_down(MouseButton::Right, |_, _, cx| cx.stop_propagation())
            .on_mouse_down(MouseButton::Middle, |_, _, cx| cx.stop_propagation())
            .on_scroll(|_, _, cx| cx.stop_propagation())
            .child(
                canvas(
                    |_, _, _| {},
                    move |bounds, _, window, _| {
                        paint_spot(bounds, spot, look, dragging, accent, square, window)
                    },
                )
                .absolute()
                .inset_0(),
            )
            .child(card)
            .into_any_element()
    }
}

/// The tour's own buttons.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Button {
    Skip,
    Back,
    Next,
}

/// The dimming, with a hole where the step is about and a frame around it; or a ring alone.
fn paint_spot(
    whole: Bounds<Pixels>,
    spot: Bounds<Pixels>,
    look: Option<Look>,
    dragging: Option<f32>,
    accent: Hsla,
    square: bool,
    window: &mut Window,
) {
    let dim = gpui::hsla(0.0, 0.0, 0.0, if square { 0.45 } else { 0.5 });
    let Some(look) = look else {
        window.paint_quad(gpui::fill(whole, dim));
        return;
    };
    let spot = match dragging {
        Some(open) => Bounds::new(
            spot.origin,
            size(spot.size.width * open, spot.size.height * open),
        ),
        None => spot,
    };
    let radius = px(if square { 0.0 } else { 8.0 });
    if look != Look::Ring {
        // (The rest of the window, around a hole with rounded corners: drawn as one, the hole left out.)
        let mut path = PathBuilder::fill();
        path.add_polygon(
            &[
                whole.origin,
                whole.top_right(),
                whole.bottom_right(),
                whole.bottom_left(),
            ],
            true,
        );
        rounded(
            &mut path,
            spot,
            radius
                .min(spot.size.width / 2.0)
                .min(spot.size.height / 2.0),
        );
        if let Ok(path) = path.build() {
            window.paint_path(path, dim);
        }
    } else {
        window.paint_quad(quad(
            spot.dilate(px(4.0)),
            Corners::all(radius + px(4.0)),
            gpui::transparent_black(),
            px(4.0),
            accent.opacity(0.3),
            BorderStyle::Solid,
        ));
    }
    let width = if look == Look::Drag { 1.0 } else { 2.0 };
    window.paint_quad(quad(
        spot.dilate(px(width)),
        Corners::all(radius + px(width)),
        gpui::transparent_black(),
        px(width),
        accent,
        BorderStyle::Solid,
    ));
}

/// A rectangle with rounded corners, as a closed part of the path.
pub fn rounded(path: &mut PathBuilder, at: Bounds<Pixels>, radius: Pixels) {
    let radius = radius.max(px(0.0));
    let (left, top) = (at.origin.x, at.origin.y);
    let (right, bottom) = (at.right(), at.bottom());
    let radii = point(radius, radius);
    path.move_to(point(left + radius, top));
    path.line_to(point(right - radius, top));
    path.arc_to(radii, px(0.0), false, true, point(right, top + radius));
    path.line_to(point(right, bottom - radius));
    path.arc_to(radii, px(0.0), false, true, point(right - radius, bottom));
    path.line_to(point(left + radius, bottom));
    path.arc_to(radii, px(0.0), false, true, point(left, bottom - radius));
    path.line_to(point(left, top + radius));
    path.arc_to(radii, px(0.0), false, true, point(left + radius, top));
    path.close();
}

/// Where what is said goes: under what it is about, or over it, or beside it, wherever it fits whole;
/// failing that, at the edge where there is most room. Always in the window.
fn saying_at(
    place: Option<Bounds<Pixels>>,
    said: gpui::Size<Pixels>,
    room: gpui::Size<Pixels>,
) -> Point<Pixels> {
    let (width, height) = (said.width.as_f32(), said.height.as_f32());
    let (room_x, room_y) = (room.width.as_f32(), room.height.as_f32());
    let within = |left: f32, top: f32| {
        point(
            px(left.min(room_x - width - EDGE).max(EDGE)),
            px(top.min(room_y - height - EDGE).max(EDGE)),
        )
    };
    let Some(place) = place else {
        return within((room_x - width) / 2.0, (room_y - height) / 2.0);
    };
    let (left, top) = (place.origin.x.as_f32(), place.origin.y.as_f32());
    let (across, down) = (place.size.width.as_f32(), place.size.height.as_f32());
    let middle = left + across / 2.0 - width / 2.0;
    let centre = top + down / 2.0 - height / 2.0;
    let below = room_y - (top + down) - GAP - EDGE;
    let above = top - GAP - EDGE;
    let right = room_x - (left + across) - GAP - EDGE;
    let beside = left - GAP - EDGE;
    if below >= height {
        within(middle, top + down + GAP)
    } else if above >= height {
        within(middle, top - GAP - height)
    } else if right >= width {
        within(left + across + GAP, centre)
    } else if beside >= width {
        within(left - GAP - width, centre)
    } else if below >= above {
        within(middle, room_y - height - EDGE)
    } else {
        within(middle, EDGE)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bounds(x: f32, y: f32, width: f32, height: f32) -> Bounds<Pixels> {
        Bounds::new(point(px(x), px(y)), size(px(width), px(height)))
    }

    #[test]
    fn what_is_said_goes_where_it_fits() {
        let room = size(px(800.0), px(600.0));
        let said = size(px(200.0), px(100.0));
        // Below, in the middle of what it is about.
        assert_eq!(
            saying_at(Some(bounds(300.0, 100.0, 200.0, 50.0)), said, room),
            point(px(300.0), px(160.0))
        );
        // Over it, when there is no room below.
        assert_eq!(
            saying_at(Some(bounds(300.0, 450.0, 200.0, 100.0)), said, room),
            point(px(300.0), px(340.0))
        );
        // In the middle of the window, about nothing.
        assert_eq!(saying_at(None, said, room), point(px(300.0), px(250.0)));
        // Never out of the window.
        let at = saying_at(Some(bounds(-50.0, 100.0, 60.0, 20.0)), said, room);
        assert_eq!(at.x, px(EDGE));
    }

    #[test]
    fn the_steps_go_forward_and_back_and_end() {
        let steps = vec![Step::new(At::Nothing, "one"), Step::new(At::Nothing, "two")];
        let mut tour = Tour::new(steps, false, false);
        assert_eq!(tour.back(), Asked::Moved);
        assert_eq!(tour.at(), 0);
        assert_eq!(tour.next(), Asked::Moved);
        assert_eq!(tour.at(), 1);
        assert_eq!(tour.next(), Asked::End);
        assert_eq!(tour.buttons(), vec![Button::Back, Button::Next]);
        tour.back();
        assert_eq!(tour.buttons(), vec![Button::Skip, Button::Next]);
    }
}
