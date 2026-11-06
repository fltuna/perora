//! The selection screen: the moment the shortcut was pressed, held still on every monitor, for the
//! user to say what of it to keep.
//!
//! One borderless window per monitor shows that monitor's part of the capture, dimmed, with what
//! would be chosen left bright. They all share one `Selection`, which feeds what the pointer does into
//! `perora_core::selection::Session`: what a press or a drag means is decided there, in the physical
//! pixels of the whole virtual screen. So a drag can start on one monitor and end on another, and
//! monitors of different scaling need no arithmetic here beyond drawing.
//!
//! Where the pointer is comes from the window's own events while it is over the window's monitor (as
//! since 0.1.7: a VR headset's pointer, as Virtual Desktop sends it, leaves the system's where the
//! press began). Off it, a press held on into another monitor, the system is asked.

use std::sync::Arc;
use std::time::{Duration, Instant};

use gpui::{
    AnyWindowHandle, App, AppContext, Bounds, Context, CursorStyle, DispatchPhase, Entity,
    FocusHandle, Hsla, InteractiveElement, IntoElement, KeyDownEvent, MouseButton, MouseDownEvent,
    MouseMoveEvent, MouseUpEvent, ParentElement, Pixels, Render, RenderImage, Styled, Subscription,
    WeakEntity, Window, WindowBackgroundAppearance, WindowBounds, WindowKind, WindowOptions, canvas,
    div, img, point, px, rgba, size,
};
use perora_core::geometry::{Point, Rect};
use perora_core::platform::{self, Desktop, Frame, Monitor, ScreenLayout, WindowId};
use perora_core::selection::{Choice, Mode, Session, without_edge};

use crate::{capture, hwnd};

/// A window is shown once it has drawn the capture. One that has not by now is shown anyway: a
/// screen that never appears is worse than one that flickers (as in the Tauri version).
const SHOW_ANYWAY_AFTER: Duration = Duration::from_millis(600);

/// What was taken at the press of the shortcut, for the selection screen.
pub struct Taken {
    pub still: Arc<Frame>,
    pub layout: ScreenLayout,
    /// The windows that can be chosen, front to back.
    pub windows: Vec<platform::Window>,
    pub pointer: Point,
    /// Each monitor's part of the still, ready to be drawn; `None` for one that could not be cut.
    pub parts: Vec<Option<Arc<RenderImage>>>,
}

/// What every monitor's window shares.
pub struct Selection {
    pub id: u64,
    session: Session,
    still: Arc<Frame>,
    /// The windows over the monitors, as far as they opened.
    overlays: Vec<AnyWindowHandle>,
    handles: Vec<WindowId>,
    /// Chosen, given up, or taken anew: nothing more is done with it.
    finished: bool,
    /// When the shortcut was pressed, for the times in the log.
    asked: Instant,
}

impl Selection {
    /// What the choice is of the capture: the area, or the window (without its edge, where what is
    /// behind it showed through). `None` when that is all off the screen.
    fn frame_of(&self, choice: &Choice) -> Option<Frame> {
        match choice {
            Choice::Area(area) => self.still.crop(*area),
            Choice::Window(index) => {
                let window = self.session.windows().get(*index)?;
                self.still
                    .crop(without_edge(window.bounds, self.session.layout()))
            }
        }
    }
}

/// Puts the selection screen up over every monitor, from what was taken. `None` when no window
/// could be opened: then there is nothing to choose on.
pub fn open(id: u64, taken: Taken, mode: Mode, asked: Instant, cx: &mut App) -> Option<Entity<Selection>> {
    let Taken {
        still,
        layout,
        windows,
        pointer,
        parts,
    } = taken;
    // The monitor the pointer was on at the start: where the mode bar is, and the keyboard.
    let home = layout
        .monitors
        .iter()
        .position(|monitor| monitor.bounds.contains(pointer))
        .or_else(|| layout.monitors.iter().position(|monitor| monitor.primary))
        .unwrap_or(0);
    let session = Session::new(layout.clone(), windows, mode, pointer);
    let selection = cx.new(|_| Selection {
        id,
        session,
        still,
        overlays: Vec::new(),
        handles: Vec::new(),
        finished: false,
        asked,
    });

    let displays = cx.displays();
    for (index, monitor) in layout.monitors.iter().enumerate() {
        let Some(image) = parts.get(index).cloned().flatten() else {
            continue;
        };
        // GPUI places a window in the logical pixels of its display: the physical corner divided by
        // that display's own scale.
        let Some(display) = displays.iter().find(|display| {
            let origin = display.bounds().origin;
            (f64::from(origin.x.as_f32()) * monitor.scale).round() as i32 == monitor.bounds.x
                && (f64::from(origin.y.as_f32()) * monitor.scale).round() as i32
                    == monitor.bounds.y
        }) else {
            eprintln!("selection {id}: no display of GPUI's for {}", monitor.name);
            continue;
        };
        let is_home = index == home;
        let options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(display.bounds())),
            titlebar: None,
            focus: is_home,
            show: true,
            kind: WindowKind::PopUp,
            is_movable: false,
            is_resizable: false,
            is_minimizable: false,
            display_id: Some(display.id()),
            window_background: WindowBackgroundAppearance::Opaque,
            ..Default::default()
        };
        let shared = selection.clone();
        let monitor = monitor.clone();
        let opened = cx.open_window(options, move |window, cx| {
            if let Some(handle) = hwnd::of(window) {
                // The screen comes and goes at once: a fade would show the desktop moving under a
                // still of itself. And it is not seen before it has drawn the still.
                platform::without_transitions(handle);
                platform::set_cloaked(handle, true);
            }
            // Alt + F4 gives up, as Esc does.
            let closing = shared.downgrade();
            window.on_window_should_close(cx, move |_, cx| {
                if let Some(selection) = closing.upgrade() {
                    cx.defer(move |cx| finish(&selection, None, cx));
                }
                false
            });
            cx.new(|cx| Overlay::new(shared, index, monitor, image, is_home, window, cx))
        });
        match opened {
            Ok(handle) => {
                let handle: AnyWindowHandle = handle.into();
                let id = handle
                    .update(cx, |_, window, _| hwnd::of(window))
                    .ok()
                    .flatten();
                selection.update(cx, |selection, _| {
                    selection.overlays.push(handle);
                    selection.handles.extend(id);
                });
                if is_home {
                    let _ = handle.update(cx, |_, window, _| window.activate_window());
                }
            }
            Err(error) => eprintln!("selection {id}: a window was not opened: {error}"),
        }
    }
    if selection.read(cx).overlays.is_empty() {
        return None;
    }
    cx.activate(true);

    let shown_anyway = selection.downgrade();
    cx.spawn(async move |cx| {
        cx.background_executor().timer(SHOW_ANYWAY_AFTER).await;
        let _ = shown_anyway.update(cx, |selection, _| {
            if !selection.finished {
                for &handle in &selection.handles {
                    platform::set_cloaked(handle, false);
                }
            }
        });
    })
    .detach();
    Some(selection)
}

/// Out of sight at once, to be taken anew: the shortcut was pressed again while it was up. Nothing
/// more is done with it; `close` takes its windows away.
pub fn put_away(selection: &Entity<Selection>, cx: &mut App) {
    selection.update(cx, |selection, _| {
        selection.finished = true;
        for &handle in &selection.handles {
            hwnd::hide(handle);
        }
    });
}

pub fn close(selection: &Entity<Selection>, cx: &mut App) {
    let overlays = selection.update(cx, |selection, _| {
        selection.finished = true;
        selection.handles.clear();
        std::mem::take(&mut selection.overlays)
    });
    for overlay in overlays {
        let _ = overlay.update(cx, |_, window, _| window.remove_window());
    }
}

/// The user decided (`None`: gave up): the screen comes down and what was chosen goes on.
pub fn finish(selection: &Entity<Selection>, choice: Option<Choice>, cx: &mut App) {
    let chosen = selection.update(cx, |selection, _| {
        if selection.finished {
            return None;
        }
        selection.finished = true;
        // Out of sight first: closing takes a turn of the message loop.
        for &handle in &selection.handles {
            hwnd::hide(handle);
        }
        Some(choice.and_then(|choice| selection.frame_of(&choice)))
    });
    let Some(frame) = chosen else {
        return;
    };
    close(selection, cx);
    capture::ended(selection, cx);
    // (A choice all off the screen: the screen just comes down.)
    if let Some(frame) = frame {
        capture::deliver(frame, cx);
    }
}

fn set_mode(selection: &Entity<Selection>, mode: Mode, cx: &mut App) {
    let choice = selection.update(cx, |selection, cx| {
        if selection.finished {
            return None;
        }
        if let Some(choice) = selection.session.set_mode(mode) {
            return Some(choice);
        }
        // The pointer has not moved, but what it points at means something else now.
        if let Ok(pointer) = platform::desktop().pointer() {
            selection.session.pointer_moved(pointer);
        }
        cx.notify();
        None
    });
    if let Some(choice) = choice {
        finish(selection, Some(choice), cx);
    }
}

#[derive(Clone, Copy)]
enum Press {
    Down,
    Moved,
    Up,
}

/// The selection screen over one monitor.
struct Overlay {
    selection: Entity<Selection>,
    monitor: usize,
    /// Its monitor, as the system has it (physical pixels).
    on: Monitor,
    still: Arc<RenderImage>,
    focus: FocusHandle,
    home: bool,
    drawn: bool,
    _observing: Subscription,
}

/// The shade over what would not be taken.
const SHADE: u32 = 0x0000_0073; // rgb(0 0 0 / 45%)
/// The frame around what would be.
const CHOSEN: u32 = 0x4f8c_ffff;

impl Overlay {
    fn new(
        selection: Entity<Selection>,
        monitor: usize,
        on: Monitor,
        still: Arc<RenderImage>,
        home: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let observing = cx.observe(&selection, |_, _, cx| cx.notify());
        let focus = cx.focus_handle();
        // Only the window the pointer started on hears keys.
        if home {
            focus.focus(window, cx);
        }
        Self {
            selection,
            monitor,
            on,
            still,
            focus,
            home,
            drawn: false,
            _observing: observing,
        }
    }

    /// From the window's logical pixels to the virtual screen's physical ones. Off this window's
    /// monitor (the button held down and taken to another), this monitor's scale says nothing of
    /// where that is, so the system is asked.
    fn screen_at(&self, position: gpui::Point<Pixels>, window: &Window) -> Point {
        let scale = window.scale_factor();
        let x = (position.x.as_f32() * scale).round() as i32;
        let y = (position.y.as_f32() * scale).round() as i32;
        let bounds = self.on.bounds;
        let own = Point::new(bounds.x + x, bounds.y + y);
        if bounds.contains(own) {
            own
        } else {
            platform::desktop().pointer().unwrap_or(own)
        }
    }

    fn press(&mut self, press: Press, position: gpui::Point<Pixels>, window: &Window, cx: &mut Context<Self>) {
        let at = self.screen_at(position, window);
        let choice = self.selection.update(cx, |selection, cx| {
            if selection.finished {
                return None;
            }
            let before = selection.session.highlight();
            let choice = match press {
                Press::Down => {
                    selection.session.pointer_down(at);
                    None
                }
                Press::Moved => {
                    selection.session.pointer_moved(at);
                    None
                }
                Press::Up => selection.session.pointer_up(at),
            };
            if choice.is_none() && selection.session.highlight() != before {
                cx.notify();
            }
            choice
        });
        if let Some(choice) = choice {
            let selection = self.selection.clone();
            cx.defer(move |cx| finish(&selection, Some(choice), cx));
        }
    }

    /// A rectangle of the virtual screen in this window's logical pixels, also where it reaches
    /// beyond the window.
    fn local(&self, area: Rect, scale: f32) -> Bounds<Pixels> {
        Bounds {
            origin: point(
                px((area.x - self.on.bounds.x) as f32 / scale),
                px((area.y - self.on.bounds.y) as f32 / scale),
            ),
            size: size(px(area.width as f32 / scale), px(area.height as f32 / scale)),
        }
    }

    fn on_key(&mut self, event: &KeyDownEvent, cx: &mut Context<Self>) {
        if event.is_held {
            return;
        }
        let keystroke = &event.keystroke;
        let selection = self.selection.clone();
        if keystroke.key == "escape" {
            cx.defer(move |cx| finish(&selection, None, cx));
            return;
        }
        if keystroke.modifiers.control || keystroke.modifiers.alt {
            return;
        }
        let picked = keystroke
            .key
            .parse::<usize>()
            .ok()
            .and_then(|number| number.checked_sub(1))
            .and_then(|index| Mode::ALL.get(index).copied());
        if let Some(mode) = picked {
            cx.defer(move |cx| set_mode(&selection, mode, cx));
        }
    }
}

impl Render for Overlay {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let scale = window.scale_factor();
        let viewport = window.viewport_size();
        let selection = self.selection.read(cx);
        let highlight = selection.session.highlight();
        let mode = selection.session.mode();

        if !self.drawn {
            self.drawn = true;
            // Seen once the still is drawn: never an empty window first.
            let (asked, id, monitor) = (selection.asked, selection.id, self.monitor);
            window.on_next_frame(move |window, _| {
                if let Some(handle) = hwnd::of(window) {
                    platform::set_cloaked(handle, false);
                }
                eprintln!(
                    "selection {id}: monitor {monitor} shown {:.0} ms after the shortcut",
                    asked.elapsed().as_secs_f64() * 1000.0
                );
            });
        }

        let cursor = match mode {
            Mode::Monitor => CursorStyle::PointingHand,
            Mode::Window => CursorStyle::Arrow,
            _ => CursorStyle::Crosshair,
        };
        let shade: Hsla = rgba(SHADE).into();
        let mut root = div()
            .id("selection")
            .size_full()
            .relative()
            .overflow_hidden()
            .bg(gpui::black())
            .cursor(cursor)
            .child(img(self.still.clone()).absolute().top_0().left_0().size_full());
        if self.home {
            root = root
                .track_focus(&self.focus)
                .on_key_down(cx.listener(|this, event: &KeyDownEvent, _, cx| this.on_key(event, cx)));
        }

        match highlight {
            Some(highlight) => {
                let area = self.local(highlight.area, scale);
                let (left, top) = (area.origin.x, area.origin.y);
                let (right, bottom) = (left + area.size.width, top + area.size.height);
                let clamp = |value: Pixels, most: Pixels| value.max(px(0.0)).min(most);
                let (inner_left, inner_right) = (clamp(left, viewport.width), clamp(right, viewport.width));
                let (inner_top, inner_bottom) = (clamp(top, viewport.height), clamp(bottom, viewport.height));
                // The shade everywhere but on what would be taken.
                root = root
                    .child(div().absolute().left_0().top_0().w_full().h(inner_top).bg(shade))
                    .child(
                        div()
                            .absolute()
                            .left_0()
                            .top(inner_bottom)
                            .w_full()
                            .h(viewport.height - inner_bottom)
                            .bg(shade),
                    )
                    .child(
                        div()
                            .absolute()
                            .left_0()
                            .top(inner_top)
                            .w(inner_left)
                            .h(inner_bottom - inner_top)
                            .bg(shade),
                    )
                    .child(
                        div()
                            .absolute()
                            .left(inner_right)
                            .top(inner_top)
                            .w(viewport.width - inner_right)
                            .h(inner_bottom - inner_top)
                            .bg(shade),
                    );
                // Its frame: 2 px across its edge; 1 px inside it while dragged (as the outline of
                // the Tauri version, offset by -1 px).
                let frame = if highlight.dragging {
                    div().absolute().left(left).top(top).w(area.size.width).h(area.size.height).border_1()
                } else {
                    div()
                        .absolute()
                        .left(left - px(1.0))
                        .top(top - px(1.0))
                        .w(area.size.width + px(2.0))
                        .h(area.size.height + px(2.0))
                        .border_2()
                };
                root = root.child(frame.border_color(rgba(CHOSEN)));
                // Its size while it is dragged out, in pixels of the screen, under its left corner.
                if highlight.dragging {
                    root = root.child(
                        div()
                            .absolute()
                            .left(left)
                            .top(bottom + px(6.0))
                            .px(px(6.0))
                            .py(px(2.0))
                            .rounded(px(4.0))
                            .bg(rgba(0x0000_00bf))
                            .text_color(gpui::white())
                            .text_size(px(12.0))
                            .whitespace_nowrap()
                            .child(format!("{} × {}", highlight.area.width, highlight.area.height)),
                    );
                }
            }
            None => root = root.child(div().absolute().size_full().bg(shade)),
        }

        // Every mouse event of this window, over whatever it is: while the button is held, the
        // window that took the press goes on getting them, also off its monitor.
        let this: WeakEntity<Self> = cx.entity().downgrade();
        root.child(
            canvas(
                |_, _, _| {},
                move |_, _, window, _| {
                    let down = this.clone();
                    window.on_mouse_event(move |event: &MouseDownEvent, phase, window, cx| {
                        if phase == DispatchPhase::Bubble && event.button == MouseButton::Left {
                            let _ = down.update(cx, |this, cx| {
                                this.press(Press::Down, event.position, window, cx)
                            });
                        }
                    });
                    let moved = this.clone();
                    window.on_mouse_event(move |event: &MouseMoveEvent, phase, window, cx| {
                        if phase == DispatchPhase::Bubble {
                            let _ = moved.update(cx, |this, cx| {
                                this.press(Press::Moved, event.position, window, cx)
                            });
                        }
                    });
                    let up = this.clone();
                    window.on_mouse_event(move |event: &MouseUpEvent, phase, window, cx| {
                        if phase == DispatchPhase::Bubble && event.button == MouseButton::Left {
                            let _ = up.update(cx, |this, cx| {
                                this.press(Press::Up, event.position, window, cx)
                            });
                        }
                    });
                },
            )
            .absolute()
            .size_full(),
        )
    }
}

/// A monitor's part of the still, as GPUI draws pictures: blue first.
pub fn drawable(part: Frame) -> Option<Arc<RenderImage>> {
    let (width, height) = (part.area.width, part.area.height);
    let mut pixels = part.rgba;
    for pixel in pixels.chunks_exact_mut(4) {
        pixel.swap(0, 2);
    }
    let buffer = image::RgbaImage::from_raw(width, height, pixels)?;
    Some(Arc::new(RenderImage::new(vec![image::Frame::new(buffer)])))
}
