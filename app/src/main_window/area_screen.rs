//! The screen on which the part of the monitor that brings the selection screen's mode bar back is
//! set (the Tauri version's `reveal.tsx`, `zone.tsx` and `reveal.rs`): opened from the capture page,
//! over the monitor the main window is on, with a still of it behind, dimmed, and the bar drawn where
//! the selection screen has it. The part is drawn and adjusted there as the editor's crop is (owner,
//! 2026-11-02: 「切り抜きツールみたいな感じに調整もできるようにしておきたい」): drag outside it to draw
//! a new one, pull its edges and corners, drag it by its inside to move it; the arrows move it, with
//! Shift they make it larger or smaller.
//!
//! The part is set on a 16:9 screen (the whole monitor when it is 16:9, else as large as fits in the
//! middle), and stretched on monitors of other proportions (「比率は16:9基準で引き伸ばされるよって
//! 記載しておいてほしい」). Nothing is set until it is saved (Enter); Esc, the Cancel button and Alt+F4
//! give up. The main window steps aside meanwhile, as for a capture, and comes back after.

use std::sync::Arc;
use std::time::Duration;

use gpui::prelude::FluentBuilder;
use gpui::{
    App, AppContext, BoxShadow, Context, CursorStyle, DispatchPhase, FocusHandle, FontWeight,
    Global, Hsla, InteractiveElement, IntoElement, KeyDownEvent, MouseButton, MouseDownEvent,
    MouseMoveEvent, MouseUpEvent, ObjectFit, ParentElement, Pixels, Point, Render, RenderImage,
    SharedString, StatefulInteractiveElement, Styled, StyledImage, Window,
    WindowBackgroundAppearance, WindowBounds, WindowHandle, WindowKind, WindowOptions, canvas, div,
    img, point, px, rgba,
};
use perora_core::language::Language;
use perora_core::platform::{self, Desktop, Frame, Monitor, ScreenCapture, WindowId};
use perora_core::selection::Mode;
use perora_core::settings::Zone;

use super::reveal_area::{zone_edge, zone_fill};
use super::system;
use crate::state::Perora;
use crate::{display, hwnd};

/// How long the main window takes to be off the screen once hidden, before the still is taken.
const SETTLE: Duration = Duration::from_millis(80);
/// How near an edge of the part, in pixels, a press takes hold of that edge.
const NEAR: f32 = 10.0;
/// The selection screen's colour of what is not chosen over the still (`rgb(0 0 0 / 45%)`).
const SHADE: u32 = 0x0000_0073;
/// The selection screen's bar: where it is.
const BAR_TOP: f32 = 24.0;

/// Whether the screen is up or on its way, and what to bring back when it goes.
#[derive(Default)]
struct Revealing {
    opening: bool,
    window: Option<WindowHandle<AreaScreen>>,
    /// The main window, put out of the way meanwhile.
    aside: Option<WindowId>,
}

impl Global for Revealing {}

/// Whether the screen is up, or on its way: nothing is to be captured meanwhile (it would take this
/// screen, and bring the main window back over it).
pub fn is_up(cx: &App) -> bool {
    cx.try_global::<Revealing>()
        .is_some_and(|revealing| revealing.opening || revealing.window.is_some())
}

/// From the settings: the screen comes up. Once: pressed again while it is coming or up, nothing
/// more happens.
pub fn edit(cx: &mut App) {
    // (Nor while a capture is under way: the screen would be in it.)
    if is_up(cx) || crate::capture::busy(cx) || crate::capture::current(cx).is_some() {
        return;
    }
    // (Over the whole monitor, it would take the screen from whoever looks at the settings, which
    // nothing that is only looked at is to do.)
    if system::dry() {
        eprintln!("dry: the screen for the bar's area would come up");
        return;
    }
    cx.default_global::<Revealing>().opening = true;
    // The monitor the main window is on (its middle), else the pointer's, else the first.
    let middle = super::on_screen_now(cx).and_then(hwnd::rect).map(|rect| {
        perora_core::geometry::Point::new(
            rect.x + rect.width as i32 / 2,
            rect.y + rect.height as i32 / 2,
        )
    });
    let aside = super::step_aside(cx);
    cx.spawn(async move |cx| {
        if aside.is_some() {
            cx.background_executor().timer(SETTLE).await;
        }
        let taken = cx
            .background_executor()
            .spawn(async move { still(middle) })
            .await;
        cx.update(|cx| {
            cx.default_global::<Revealing>().opening = false;
            let opened = match taken {
                Ok((monitor, frame)) => open(&monitor, frame, cx),
                Err(error) => Err(error),
            };
            match opened {
                Ok(window) => {
                    let revealing = cx.default_global::<Revealing>();
                    revealing.window = Some(window);
                    revealing.aside = aside;
                }
                // (A monitor that could not be taken is not shown black: the main window is back.)
                Err(error) => {
                    eprintln!("the screen for the bar's area: {error}");
                    if let Some(aside) = aside {
                        super::come_back(aside);
                    }
                }
            }
        });
    })
    .detach();
}

/// The monitor the screen is to be on, and a still of it.
fn still(middle: Option<perora_core::geometry::Point>) -> Result<(Monitor, Frame), String> {
    let capture = platform::screen_capture();
    let layout = capture.layout().map_err(|error| error.to_string())?;
    let pointer = platform::desktop().pointer().ok();
    let on = |at: Option<perora_core::geometry::Point>| {
        layout
            .monitors
            .iter()
            .find(|monitor| at.is_some_and(|at| monitor.bounds.contains(at)))
    };
    let monitor = on(middle)
        .or_else(|| on(pointer))
        .or_else(|| layout.monitors.first())
        .ok_or("no monitor")?
        .clone();
    let still = capture
        .capture_all()
        .map_err(|error| error.to_string())?
        .crop(monitor.bounds)
        .ok_or("the monitor is not in the capture")?;
    Ok((monitor, still))
}

fn open(monitor: &Monitor, still: Frame, cx: &mut App) -> Result<WindowHandle<AreaScreen>, String> {
    let picture = crate::selection::drawable(still).ok_or("the still could not be drawn")?;
    let display = display::of(monitor, cx).ok_or("no display for the monitor")?;
    let zone = Perora::get(cx).settings().bar_reveal_area;
    let options = WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(display.bounds())),
        titlebar: None,
        focus: true,
        show: true,
        kind: WindowKind::PopUp,
        is_movable: false,
        is_resizable: false,
        is_minimizable: false,
        display_id: Some(display.id()),
        window_background: WindowBackgroundAppearance::Opaque,
        ..Default::default()
    };
    cx.open_window(options, |window, cx| {
        if let Some(handle) = hwnd::of(window) {
            // At once, and not before it has drawn the still.
            platform::without_transitions(handle);
            platform::set_cloaked(handle, true);
        }
        window.on_next_frame(|window, _| {
            if let Some(handle) = hwnd::of(window) {
                platform::set_cloaked(handle, false);
            }
            window.activate_window();
        });
        // Alt + F4 gives up, as Esc does.
        window.on_window_should_close(cx, |_, cx| {
            cx.defer(|cx| {
                if let Some(window) = cx.default_global::<Revealing>().window {
                    let _ = window.update(cx, |screen, window, cx| screen.done(None, window, cx));
                }
            });
            false
        });
        cx.new(|cx| {
            let screen = AreaScreen::new(picture, zone, cx);
            window.focus(&screen.focus, cx);
            screen
        })
    })
    .map_err(|error| error.to_string())
}

/// A place on the screen, as shares of the 16:9 stage's width and height.
type Spot = (f64, f64);

/// What a press on the stage does to the part: pull an edge or a corner of it (which way on each
/// side: -1 the left or top one, 1 the right or bottom one), move it, or draw a new one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Grip {
    Edge(i8, i8),
    Move,
    New,
}

impl Grip {
    /// The pointer that says what a press would do.
    fn cursor(self) -> CursorStyle {
        match self {
            Grip::Move => CursorStyle::OpenHand,
            Grip::New => CursorStyle::Crosshair,
            Grip::Edge(0, _) => CursorStyle::ResizeUpDown,
            Grip::Edge(_, 0) => CursorStyle::ResizeLeftRight,
            Grip::Edge(x, y) if x == y => CursorStyle::ResizeUpLeftDownRight,
            Grip::Edge(..) => CursorStyle::ResizeUpRightDownLeft,
        }
    }
}

/// What a press at `at` (pixels from the stage's corner, the stage `size` across) takes of the part.
/// Of a part too thin for holds inside it (the strip at the top is a few pixels), an edge only from
/// outside: a press inside moves it.
fn grip_at(zone: Zone, at: (f32, f32), size: (f32, f32)) -> Grip {
    let (px, py) = at;
    let (x0, y0) = (zone.x as f32 * size.0, zone.y as f32 * size.1);
    let (x1, y1) = (
        x0 + zone.width as f32 * size.0,
        y0 + zone.height as f32 * size.1,
    );
    let across = px >= x0 - NEAR && px <= x1 + NEAR;
    let along = py >= y0 - NEAR && py <= y1 + NEAR;
    let edge = |p: f32, a: f32, b: f32| -> i8 {
        if b - a < NEAR * 3.0 {
            if p <= a && a - p <= NEAR {
                -1
            } else if p >= b && p - b <= NEAR {
                1
            } else {
                0
            }
        } else if (p - a).abs() <= NEAR {
            -1
        } else if (p - b).abs() <= NEAR {
            1
        } else {
            0
        }
    };
    let x = if along { edge(px, x0, x1) } else { 0 };
    let y = if across { edge(py, y0, y1) } else { 0 };
    if x != 0 || y != 0 {
        Grip::Edge(x, y)
    } else if px >= x0 && px <= x1 && py >= y0 && py <= y1 {
        Grip::Move
    } else {
        Grip::New
    }
}

fn between(a: Spot, b: Spot) -> Zone {
    Zone {
        x: a.0.min(b.0),
        y: a.1.min(b.1),
        width: (a.0 - b.0).abs(),
        height: (a.1 - b.1).abs(),
    }
}

/// The part as a gesture that began at `from` (on `was`) leaves it with the pointer at `at`. The edge
/// held goes as far as the pointer does (not to where it is: a press on a hold beside the edge moves
/// nothing); the one across from it stays, and passing it turns the part.
fn pulled(grip: Grip, was: Zone, from: Spot, at: Spot) -> Zone {
    let clamp = |n: f64| n.clamp(0.0, 1.0);
    match grip {
        Grip::New => between(from, at),
        Grip::Move => Zone {
            x: (was.x + at.0 - from.0).max(0.0).min(1.0 - was.width),
            y: (was.y + at.1 - from.1).max(0.0).min(1.0 - was.height),
            ..was
        },
        Grip::Edge(x, y) => {
            let (dx, dy) = (at.0 - from.0, at.1 - from.1);
            let (mut left, mut right) = (was.x, was.x + was.width);
            let (mut top, mut bottom) = (was.y, was.y + was.height);
            match x {
                -1 => left = clamp(left + dx),
                1 => right = clamp(right + dx),
                _ => {}
            }
            match y {
                -1 => top = clamp(top + dy),
                1 => bottom = clamp(bottom + dy),
                _ => {}
            }
            between((left, top), (right, bottom))
        }
    }
}

/// What was made, when it is a part to reach and not what is there already.
fn taken(made: Zone, shown: Zone) -> Option<Zone> {
    let changed = [
        (made.x, shown.x),
        (made.y, shown.y),
        (made.width, shown.width),
        (made.height, shown.height),
    ]
    .iter()
    .any(|(a, b)| (a - b).abs() > 1e-6);
    (changed && made.width > 0.005 && made.height > 0.002).then_some(made)
}

/// The part moved by an arrow (a hundredth of the monitor), or with Shift made larger or smaller to
/// the right and down.
fn keyed(was: Zone, (dx, dy): (f64, f64), larger: bool) -> Zone {
    if larger {
        Zone {
            width: (was.width + dx).max(0.01).min(1.0 - was.x),
            height: (was.height + dy).max(0.01).min(1.0 - was.y),
            ..was
        }
    } else {
        Zone {
            x: (was.x + dx).max(0.0).min(1.0 - was.width),
            y: (was.y + dy).max(0.0).min(1.0 - was.height),
            ..was
        }
    }
}

/// A gesture under way: what it holds, where it began, and the part as it was then.
struct Gesture {
    grip: Grip,
    from: Spot,
    was: Zone,
}

pub struct AreaScreen {
    still: Arc<RenderImage>,
    /// The part as it is drawn so far: set only when it is saved.
    zone: Zone,
    gesture: Option<Gesture>,
    /// The part as the gesture under way leaves it, once the pointer moved.
    draft: Option<Zone>,
    hover: Grip,
    focus: FocusHandle,
    /// Saved or given up: nothing more is done.
    done: bool,
}

impl AreaScreen {
    pub fn new(still: Arc<RenderImage>, zone: Zone, cx: &mut Context<Self>) -> Self {
        Self {
            still,
            zone,
            gesture: None,
            draft: None,
            hover: Grip::New,
            focus: cx.focus_handle(),
            done: false,
        }
    }

    /// The 16:9 stage: as large as fits in the window, in its middle.
    fn stage(window: &Window) -> (Point<Pixels>, (f32, f32)) {
        let viewport = window.viewport_size();
        let (wide, high) = (viewport.width.as_f32(), viewport.height.as_f32());
        let (width, height) = (wide.min(high * 16.0 / 9.0), high.min(wide * 9.0 / 16.0));
        (
            point(px((wide - width) / 2.0), px((high - height) / 2.0)),
            (width, height),
        )
    }

    fn spot(position: Point<Pixels>, window: &Window) -> Spot {
        let (corner, (width, height)) = Self::stage(window);
        (
            (((position.x - corner.x).as_f32() / width) as f64).clamp(0.0, 1.0),
            (((position.y - corner.y).as_f32() / height) as f64).clamp(0.0, 1.0),
        )
    }

    fn grip(&self, position: Point<Pixels>, window: &Window) -> Grip {
        let (corner, size) = Self::stage(window);
        let at = (
            (position.x - corner.x).as_f32(),
            (position.y - corner.y).as_f32(),
        );
        grip_at(self.zone, at, size)
    }

    fn pressed(&mut self, event: &MouseDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        window.focus(&self.focus, cx);
        // (A new one is drawn from the first move: a click leaves the one there is in sight.)
        self.gesture = Some(Gesture {
            grip: self.grip(event.position, window),
            from: Self::spot(event.position, window),
            was: self.zone,
        });
        cx.notify();
    }

    fn moved(&mut self, event: &MouseMoveEvent, window: &mut Window, cx: &mut Context<Self>) {
        match &self.gesture {
            Some(gesture) if event.pressed_button == Some(MouseButton::Left) => {
                let at = Self::spot(event.position, window);
                self.draft = Some(pulled(gesture.grip, gesture.was, gesture.from, at));
            }
            // (Let go where the window did not hear it.)
            Some(_) => {
                self.gesture = None;
                self.draft = None;
            }
            None => self.hover = self.grip(event.position, window),
        }
        cx.notify();
    }

    fn let_go(&mut self, event: &MouseUpEvent, window: &mut Window, cx: &mut Context<Self>) {
        let Some(gesture) = self.gesture.take() else {
            return;
        };
        // (Where it was let go, not the last move drawn. A click, a part too thin to reach, or one
        // that did not change, sets nothing.)
        if self.draft.take().is_some() {
            let at = Self::spot(event.position, window);
            if let Some(made) = taken(
                pulled(gesture.grip, gesture.was, gesture.from, at),
                self.zone,
            ) {
                self.zone = made;
            }
        }
        cx.notify();
    }

    fn key_down(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let keystroke = &event.keystroke;
        let step = match keystroke.key.as_str() {
            "left" => Some((-0.01, 0.0)),
            "right" => Some((0.01, 0.0)),
            "up" => Some((0.0, -0.01)),
            "down" => Some((0.0, 0.01)),
            _ => None,
        };
        if let Some(step) = step {
            if let Some(made) = taken(keyed(self.zone, step, keystroke.modifiers.shift), self.zone)
            {
                self.zone = made;
                cx.notify();
            }
            return;
        }
        // (Held, the arrows go on; Enter and Esc count once.)
        if event.is_held {
            return;
        }
        match keystroke.key.as_str() {
            // (On a button, Enter is the button's: Cancel is not a save.)
            "enter" if self.focus.is_focused(window) => self.done(Some(self.zone), window, cx),
            "escape" => self.done(None, window, cx),
            _ => {}
        }
    }

    /// Done with the screen: the part is set when one is given (saved), not when none is (given
    /// up). Once.
    fn done(&mut self, zone: Option<Zone>, window: &mut Window, cx: &mut Context<Self>) {
        if self.done {
            return;
        }
        self.done = true;
        if let Some(zone) = zone.and_then(|zone| Zone::from_value(&zone.to_value())) {
            system::set(cx, |settings| settings.bar_reveal_area = zone);
        }
        let revealing = cx.default_global::<Revealing>();
        revealing.window = None;
        let aside = revealing.aside.take();
        if let Some(handle) = hwnd::of(window) {
            hwnd::hide(handle);
        }
        window.remove_window();
        if let Some(aside) = aside {
            super::come_back(aside);
        }
    }

    /// The bar as the selection screen has it, where it has it: to set the part by.
    fn bar(language: &Language, theme: &perora_ui::Theme) -> impl IntoElement + use<> {
        let (accent, on_accent) = (theme.accent_dark, theme.accent_dark_text);
        let key = |number: usize, colour: Hsla| {
            div()
                .min_w(px(18.0))
                .px(px(4.0))
                .rounded(px(4.0))
                .border_1()
                .border_color(colour)
                .opacity(0.7)
                .text_size(px(11.0))
                .flex()
                .justify_center()
                .child(crate::bar::digit(number))
        };
        let modes = Mode::ALL.iter().enumerate().map(|(index, mode)| {
            let current = index == 0;
            let colour: Hsla = if current {
                on_accent
            } else {
                rgba(0xeceef2ff).into()
            };
            div()
                .flex()
                .items_center()
                .gap(px(6.0))
                .h(px(32.0))
                .px(px(12.0))
                .rounded(px(6.0))
                .whitespace_nowrap()
                .text_color(colour)
                .when(current, |button| button.bg(accent))
                .child(key(index + 1, colour))
                .child(language.text(&format!("overlay.mode.{}", mode.name()), &[]))
        });
        div()
            .absolute()
            .left_0()
            .top(px(BAR_TOP))
            .w_full()
            .flex()
            .justify_center()
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
                        color: rgba(SHADE).into(),
                        offset: point(px(0.0), px(8.0)),
                        blur_radius: px(24.0),
                        spread_radius: px(0.0),
                        inset: false,
                    }])
                    .children(modes)
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .h(px(32.0))
                            .px(px(12.0))
                            .ml(px(6.0))
                            .whitespace_nowrap()
                            .text_color(rgba(0x9aa1_adff))
                            .child(language.text("overlay.cancel", &[])),
                    ),
            )
    }
}

impl Render for AreaScreen {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let language = Perora::get(cx).language();
        let theme = perora_ui::Theme::get(cx).clone();
        let t = |key: &str| -> SharedString { language.text(key, &[]).into() };
        let (corner, (width, height)) = Self::stage(window);
        let zone = self.draft.unwrap_or(self.zone);
        let cursor = self
            .gesture
            .as_ref()
            .map_or(self.hover, |gesture| gesture.grip)
            .cursor();
        let drawing = self.draft.is_some();
        // The part, with the holds at its corners and the middles of its edges, as the crop's.
        let (left, top) = (zone.x as f32 * width, zone.y as f32 * height);
        let (wide, high) = (
            (zone.width as f32 * width).max(2.0),
            (zone.height as f32 * height).max(2.0),
        );
        let holds = [
            (0.0, 0.0),
            (0.5, 0.0),
            (1.0, 0.0),
            (1.0, 0.5),
            (1.0, 1.0),
            (0.5, 1.0),
            (0.0, 1.0),
            (0.0, 0.5),
        ]
        .map(|(across, down)| {
            div()
                .absolute()
                .left(px(left + wide * across - 5.0))
                .top(px(top + high * down - 5.0))
                .size(px(9.0))
                .rounded(px(1.0))
                .bg(gpui::white())
                .border_1()
                .border_color(zone_edge(&theme))
        });
        let part = div()
            .absolute()
            .left(px(left))
            .top(px(top))
            .w(px(wide))
            .h(px(high))
            .bg(zone_fill(&theme))
            .border_1()
            .border_color(zone_edge(&theme))
            .when(drawing, |part| part.border_dashed());
        // (The still fills the stage, as the part is set on it: the monitor as 16:9. Beside it,
        // on a monitor that is not, black.)
        let stage = div()
            .absolute()
            .left(corner.x)
            .top(corner.y)
            .w(px(width))
            .h(px(height))
            .child(
                img(self.still.clone())
                    .absolute()
                    .top_0()
                    .left_0()
                    .size_full()
                    .object_fit(ObjectFit::Fill),
            )
            .child(div().absolute().inset_0().bg(rgba(SHADE)))
            .child(Self::bar(&language, perora_ui::Theme::get(cx)))
            .child(part)
            .children(holds);

        let button = |id: &'static str, said: SharedString, current: bool| {
            div()
                .id(id)
                .flex()
                .items_center()
                .h(px(32.0))
                .px(px(12.0))
                .rounded(px(6.0))
                .whitespace_nowrap()
                .cursor(CursorStyle::PointingHand)
                .map(|button| {
                    if current {
                        button.bg(theme.accent_dark).text_color(theme.accent_dark_text)
                    } else {
                        button
                            .text_color(rgba(0xeceef2ff))
                            .hover(|button| button.bg(rgba(0x2326_2dff)))
                    }
                })
                .child(said)
        };
        // What it is for, and saving or giving up: at the bottom, out of the way of the top.
        // (What is pressed there is not a press on the stage.)
        let tools = div()
            .absolute()
            .left_0()
            .bottom(px(32.0))
            .w_full()
            .flex()
            .justify_center()
            .child(
                div()
                    .id("tools")
                    .occlude()
                    .cursor(CursorStyle::Arrow)
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap(px(8.0))
                    .child(
                        div()
                            .max_w(px(720.0))
                            .px(px(14.0))
                            .py(px(6.0))
                            .rounded(px(10.0))
                            .bg(rgba(0x0000_00b3))
                            .text_color(gpui::white())
                            .text_center()
                            .child(t("reveal.hint")),
                    )
                    .child(
                        div()
                            .flex()
                            .gap(px(4.0))
                            .p(px(6.0))
                            .rounded(px(10.0))
                            .border_1()
                            .border_color(rgba(0x353a_44ff))
                            .bg(rgba(0x1618_1deb))
                            .child(button("save", t("reveal.save"), true).on_click(cx.listener(
                                |this, _, window, cx| this.done(Some(this.zone), window, cx),
                            )))
                            .child(
                                button("reset", t("reveal.reset"), false).on_click(cx.listener(
                                    |this, _, _, cx| {
                                        this.zone = Zone::BAR_REVEAL;
                                        cx.notify();
                                    },
                                )),
                            )
                            .child(
                                button("cancel", t("reveal.cancel"), false)
                                    .ml(px(6.0))
                                    .text_color(rgba(0x9aa1_adff))
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.done(None, window, cx)
                                    })),
                            ),
                    ),
            );

        div()
            .id("area-screen")
            .size_full()
            .relative()
            .bg(gpui::black())
            .text_color(gpui::white())
            .font_family(perora_ui::Theme::get(cx).font.clone())
            .text_size(px(14.0))
            .font_weight(FontWeight::NORMAL)
            .cursor(cursor)
            .track_focus(&self.focus)
            .on_key_down(cx.listener(Self::key_down))
            .on_mouse_down(MouseButton::Left, cx.listener(Self::pressed))
            // (Every move of the pointer over the window, over the buttons too: a part being drawn
            // follows the pointer wherever it goes, as the Tauri version's pointer capture had it.)
            .child({
                let screen = cx.entity();
                canvas(
                    |_, _, _| {},
                    move |_, _, window, _| {
                        window.on_mouse_event(move |event: &MouseMoveEvent, phase, window, cx| {
                            if phase == DispatchPhase::Bubble {
                                screen.update(cx, |screen, cx| screen.moved(event, window, cx));
                            }
                        });
                    },
                )
                .absolute()
                .size_0()
            })
            .on_mouse_up(MouseButton::Left, cx.listener(Self::let_go))
            .on_mouse_up_out(MouseButton::Left, cx.listener(Self::let_go))
            .child(stage)
            .child(tools)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const STRIP: Zone = Zone::BAR_REVEAL;

    #[test]
    fn a_press_takes_an_edge_a_corner_the_inside_or_draws_anew() {
        let wide = Zone {
            x: 0.25,
            y: 0.25,
            width: 0.5,
            height: 0.5,
        };
        let size = (1000.0, 500.0);
        // Its edges are at 250 and 750 across, 125 and 375 down.
        assert_eq!(grip_at(wide, (252.0, 300.0), size), Grip::Edge(-1, 0));
        assert_eq!(grip_at(wide, (748.0, 370.0), size), Grip::Edge(1, 1));
        assert_eq!(grip_at(wide, (500.0, 120.0), size), Grip::Edge(0, -1));
        assert_eq!(grip_at(wide, (500.0, 300.0), size), Grip::Move);
        assert_eq!(grip_at(wide, (100.0, 300.0), size), Grip::New);
        // The thin strip at the top: an edge only from outside, a press inside moves it.
        assert_eq!(grip_at(STRIP, (500.0, 2.0), size), Grip::Move);
        assert_eq!(grip_at(STRIP, (500.0, 9.0), size), Grip::Edge(0, 1));
    }

    #[test]
    fn a_pulled_edge_goes_as_far_as_the_pointer_and_turns_past_the_other() {
        let was = Zone {
            x: 0.2,
            y: 0.2,
            width: 0.4,
            height: 0.4,
        };
        let pulled_right = pulled(Grip::Edge(1, 0), was, (0.61, 0.5), (0.71, 0.5));
        assert!((pulled_right.width - 0.5).abs() < 1e-9 && pulled_right.x == 0.2);
        let turned = pulled(Grip::Edge(-1, 0), was, (0.2, 0.5), (0.8, 0.5));
        assert!((turned.x - 0.6).abs() < 1e-9 && (turned.width - 0.2).abs() < 1e-9);
        // Moved, it stays on the monitor.
        let moved = pulled(Grip::Move, was, (0.5, 0.5), (1.5, -0.5));
        assert!((moved.x - 0.6).abs() < 1e-9 && moved.y == 0.0);
        // Drawn anew, from where it was pressed.
        assert_eq!(
            pulled(Grip::New, was, (0.9, 0.9), (0.7, 0.8)),
            between((0.7, 0.8), (0.9, 0.9))
        );
    }

    #[test]
    fn what_is_set_is_a_part_to_reach_and_a_change() {
        assert_eq!(taken(STRIP, STRIP), None);
        let tiny = Zone {
            width: 0.004,
            ..STRIP
        };
        assert_eq!(taken(tiny, STRIP), None);
        let moved = keyed(STRIP, (0.01, 0.0), false);
        assert_eq!(taken(moved, STRIP), Some(moved));
        // At the top already: up does not move it.
        assert_eq!(taken(keyed(STRIP, (0.0, -0.01), false), STRIP), None);
        // Smaller, not below a hundredth.
        let smaller = keyed(STRIP, (0.0, -0.01), true);
        assert!((smaller.height - 0.01).abs() < 1e-9);
    }
}
