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
//!
//! The window of the monitor the pointer started on has the mode bar (`bar`), the list of windows of
//! the window mode (`window_list`) and the keyboard. Every window draws a + where the pointer is,
//! while it is on its monitor and not over the bar or a card of the list.
//!
//! While the screen is up, each window's own picture is taken in the background. Choosing a window
//! then gives that window alone, without whatever lay in front of it; a window that gives no picture
//! of itself is cut out of the capture of the screen instead. These pictures are taken one after the
//! other in the first moments the screen is up, so a window whose content moves (a video, a game) is
//! in its picture a few tenths of a second later than in the capture of the screen.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use gpui::{
    AnyWindowHandle, App, AppContext, Bounds, Context, CursorStyle, DispatchPhase, Entity,
    FocusHandle, Hsla, InteractiveElement, IntoElement, KeyDownEvent, MouseButton, MouseDownEvent,
    MouseMoveEvent, MouseUpEvent, ParentElement, Pixels, Render, RenderImage, Styled, Subscription,
    WeakEntity, Window, WindowBackgroundAppearance, WindowBounds, WindowHandle, WindowKind,
    WindowOptions, canvas, div, fill, img, point, px, rgba, size,
};
use perora_core::geometry::{Point, Rect};
use perora_core::language::Language;
use perora_core::platform::{self, Desktop, Frame, Monitor, ScreenCapture, ScreenLayout, WindowId};
use perora_core::selection::{Choice, Mode, Session, without_edge};

use crate::bar::{self, Bar};
use crate::state::Perora;
use crate::window_list::{self, List};
use crate::{capture, cursor, hwnd};

/// A window is shown once it has drawn the capture. One that has not by now is shown anyway: a
/// screen that never appears is worse than one that flickers (as in the Tauri version).
const SHOW_ANYWAY_AFTER: Duration = Duration::from_millis(600);

/// The pictures of single windows kept while the screen is up, at most: front to back, until this
/// many bytes are used. The rest are cut out of the capture of the screen.
const WINDOW_PICTURES_BUDGET: usize = 512 * 1024 * 1024;

/// What was taken at the press of the shortcut, for the selection screen.
pub struct Taken {
    pub still: Arc<Frame>,
    pub layout: ScreenLayout,
    /// The windows that can be chosen, front to back.
    pub windows: Vec<platform::Window>,
    pub pointer: Point,
    /// Each monitor's part of the still, ready to be drawn; `None` for one that could not be cut.
    pub parts: Vec<Option<Arc<RenderImage>>>,
    /// Perora's own among the windows (left in the picture): hidden by now, so what the capture has
    /// of them is all there is.
    pub own: Vec<WindowId>,
}

/// What every monitor's window shares.
pub struct Selection {
    pub id: u64,
    session: Session,
    still: Arc<Frame>,
    /// The monitor the pointer was on at the start: where the mode bar is, and the keyboard.
    home: usize,
    bar: Bar,
    list: List,
    /// The pictures of single windows taken so far.
    pictures: Arc<Mutex<HashMap<WindowId, Arc<Frame>>>>,
    /// Cleared as the screen comes down: what is still being made for it stops.
    up: Arc<AtomicBool>,
    /// Whether a + is drawn where the pointer is (and the system's pointer is not).
    crosshair: bool,
    /// Where the pointer was last seen, on the virtual screen (physical pixels).
    pointer: Point,
    /// Whether the button went down on the screen and has not come up: until it does, what the
    /// pointer does is the screen's, wherever it is (over the bar too).
    pressed: bool,
    language: Language,
    /// The windows over the monitors, as far as they opened.
    overlays: Vec<AnyWindowHandle>,
    handles: Vec<WindowId>,
    /// Chosen, given up, or taken anew: nothing more is done with it.
    finished: bool,
    /// When the shortcut was pressed, for the times in the log.
    asked: Instant,
}

impl Drop for Selection {
    fn drop(&mut self) {
        eprintln!("selection {}: let go of", self.id);
    }
}

impl Selection {
    /// What the choice is of the capture, and where to cut it out of: the area of the still, or the
    /// window (without its edge, where what is behind it showed through) of its own picture or of
    /// the still. `None` when that is all off the screen. (Cut out by whoever takes it on, off the
    /// UI thread: a choice across monitors is a copy of a hundred megabytes and more.)
    fn cut_of(&self, choice: &Choice) -> Option<(Arc<Frame>, Rect)> {
        let of_still = |area: Rect| {
            area.intersection(&self.still.area)
                .map(|seen| (self.still.clone(), seen))
        };
        match choice {
            Choice::Area(area) => of_still(*area),
            Choice::Window(index) => {
                let window = self.session.windows().get(*index)?;
                let picture = self
                    .pictures
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner())
                    .get(&window.id)
                    .cloned();
                eprintln!(
                    "selection {}: chose window {index} {:?} at {:?}: {}",
                    self.id,
                    window.title,
                    window.bounds,
                    if picture.is_some() {
                        "its own picture"
                    } else {
                        "cut out of the screen"
                    }
                );
                let bounds = without_edge(window.bounds, self.session.layout());
                match picture {
                    // The window alone, as it was; of it, what was on the screen, as highlighted
                    // (its own picture also has the part that hung off the edge).
                    Some(picture) => bounds
                        .intersection(&self.still.area)
                        .and_then(|seen| seen.intersection(&picture.area))
                        .map(|seen| (picture, seen)),
                    // No picture of its own (yet, or ever): what was seen of it on the screen.
                    None => of_still(bounds),
                }
            }
        }
    }

    pub fn bar(&self) -> &Bar {
        &self.bar
    }

    fn home_monitor(&self) -> &Monitor {
        &self.session.layout().monitors[self.home]
    }

    /// The pointer was seen here: the bar is told, in its window's pixels.
    fn pointer_at(&mut self, at: Point, cx: &mut Context<Self>) {
        self.pointer = at;
        let home = self.home_monitor();
        let scale = home.scale as f32;
        let viewport = size(
            px(home.bounds.width as f32 / scale),
            px(home.bounds.height as f32 / scale),
        );
        let local = home.bounds.contains(at).then(|| {
            point(
                px((at.x - home.bounds.x) as f32 / scale),
                px((at.y - home.bounds.y) as f32 / scale),
            )
        });
        self.bar.pointer_at(local, viewport);
        self.bar_changed(cx);
    }

    /// Whether the bar is to stay or go now; a wait for it to go, when there is one.
    fn bar_changed(&mut self, cx: &mut Context<Self>) {
        let dragging = self.session.highlight().is_some_and(|highlight| highlight.dragging);
        let Some((timer, seconds)) = self.bar.update(dragging) else {
            return;
        };
        cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(Duration::from_secs_f64(seconds))
                .await;
            let _ = this.update(cx, |selection, cx| {
                if selection.bar.waited(timer) {
                    cx.notify();
                }
            });
        })
        .detach();
    }
}

/// Puts the selection screen up over every monitor, from what was taken. `None` when no window
/// could be opened: then there is nothing to choose on.
/// The windows of the selection screen, opened over the monitors while the screen is taken: out of
/// sight and empty until it is there. Opening them takes as long as taking the screen does, so the
/// two are done at once (owner, 2026-11-06, of making the screen come up sooner: 「1がRIO良さそう」).
pub struct Prepared {
    windows: Vec<(Rect, WindowHandle<Shell>)>,
}

/// Opens a window over every monitor there is now, for the screen being taken.
pub fn prepare(cx: &mut App) -> Option<Prepared> {
    let layout = platform::screen_capture().layout().ok()?;
    let pointer = platform::desktop().pointer().ok();
    let home = pointer
        .and_then(|pointer| {
            layout
                .monitors
                .iter()
                .position(|monitor| monitor.bounds.contains(pointer))
        })
        .or_else(|| layout.monitors.iter().position(|monitor| monitor.primary))
        .unwrap_or(0);
    let windows = layout
        .monitors
        .iter()
        .enumerate()
        .filter_map(|(index, monitor)| {
            open_shell(monitor, index == home, cx).map(|handle| (monitor.bounds, handle))
        })
        .collect();
    Some(Prepared { windows })
}

/// The windows opened for a screen that did not come of it: closed.
pub fn discard(prepared: Prepared, cx: &mut App) {
    for (_, window) in prepared.windows {
        let _ = window.update(cx, |_, window, _| window.remove_window());
    }
}

/// A window over a monitor, out of sight (until it has drawn the screen) and empty.
fn open_shell(monitor: &Monitor, focus: bool, cx: &mut App) -> Option<WindowHandle<Shell>> {
    let display = crate::display::of(monitor, cx)?;
    let options = WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(display.bounds())),
        titlebar: None,
        focus,
        show: true,
        kind: WindowKind::PopUp,
        is_movable: false,
        is_resizable: false,
        is_minimizable: false,
        display_id: Some(display.id()),
        window_background: WindowBackgroundAppearance::Opaque,
        ..Default::default()
    };
    let opened = cx.open_window(options, |window, cx| {
        if let Some(handle) = hwnd::of(window) {
            // The screen comes and goes at once: a fade would show the desktop moving under a
            // still of itself. And it is not seen before it has drawn the still.
            platform::without_transitions(handle);
            platform::set_cloaked(handle, true);
        }
        // Alt + F4 gives up, as Esc does.
        window.on_window_should_close(cx, |_, cx| {
            if let Some(selection) = capture::current(cx) {
                cx.defer(move |cx| finish(&selection, None, cx));
            }
            false
        });
        cx.new(|_| Shell { overlay: None })
    });
    match opened {
        Ok(window) => Some(window),
        Err(error) => {
            eprintln!("the selection screen's window was not opened for {}: {error}", monitor.name);
            None
        }
    }
}

/// A window of the selection screen: empty while the screen is taken, then the screen over its
/// monitor.
pub struct Shell {
    overlay: Option<Entity<Overlay>>,
}

impl Render for Shell {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        match &self.overlay {
            Some(overlay) => overlay.clone().into_any_element(),
            None => div().size_full().bg(gpui::black()).into_any_element(),
        }
    }
}

pub fn open(
    id: u64,
    taken: Taken,
    prepared: Option<Prepared>,
    mode: Mode,
    asked: Instant,
    cx: &mut App,
) -> Option<Entity<Selection>> {
    let Taken {
        still,
        layout,
        windows,
        pointer,
        parts,
        own,
    } = taken;
    let home = layout
        .monitors
        .iter()
        .position(|monitor| monitor.bounds.contains(pointer))
        .or_else(|| layout.monitors.iter().position(|monitor| monitor.primary))
        .unwrap_or(0);
    let perora = Perora::get(cx);
    let settings = perora.settings();
    let language = perora.language();
    let session = Session::new(layout.clone(), windows, mode, pointer);
    let on_home = &layout.monitors[home];
    let scale = on_home.scale as f32;
    let list = List::new(
        session.windows(),
        on_home.bounds.width as f32 / scale,
        on_home.bounds.height as f32 / scale,
    );
    let selection = cx.new(|cx| {
        let mut selection = Selection {
            id,
            session,
            still,
            home,
            bar: Bar::new(settings.bar_hide_seconds, settings.bar_reveal_area),
            list,
            pictures: Arc::default(),
            up: Arc::new(AtomicBool::new(true)),
            crosshair: settings.show_crosshair,
            pointer,
            pressed: false,
            language,
            overlays: Vec::new(),
            handles: Vec::new(),
            finished: false,
            asked,
        };
        selection.pointer_at(pointer, cx);
        selection
    });

    // The window with the bar, as it opened (or the first that did).
    let mut opened_home: Option<(usize, AnyWindowHandle)> = None;
    let mut prepared = prepared.map(|prepared| prepared.windows).unwrap_or_default();
    for (index, monitor) in layout.monitors.iter().enumerate() {
        // The window opened for this monitor while the screen was taken, or one opened now (the
        // monitors changed meanwhile).
        let made = prepared
            .iter()
            .position(|(bounds, _)| *bounds == monitor.bounds)
            .map(|at| prepared.remove(at).1);
        let Some(image) = parts.get(index).cloned().flatten() else {
            if let Some(made) = made {
                let _ = made.update(cx, |_, window, _| window.remove_window());
            }
            continue;
        };
        let is_home = index == home;
        let Some(handle) = made.or_else(|| open_shell(monitor, is_home, cx)) else {
            eprintln!("selection {id}: a window was not opened for {}", monitor.name);
            continue;
        };
        let shared = selection.clone();
        let monitor = monitor.clone();
        let adopted = handle.update(cx, move |shell, window, cx| {
            shell.overlay = Some(cx.new(|cx| Overlay::new(shared, index, monitor, image, window, cx)));
            cx.notify();
            hwnd::of(window)
        });
        let Ok(id) = adopted else {
            continue;
        };
        let handle: AnyWindowHandle = handle.into();
        selection.update(cx, |selection, _| {
            selection.overlays.push(handle);
            selection.handles.extend(id);
            opened_home.get_or_insert((index, handle));
            if is_home {
                opened_home = Some((index, handle));
            }
        });
    }
    // (Of monitors that are gone.)
    discard(Prepared { windows: prepared }, cx);
    // The bar and the keyboard where the pointer is; on another monitor, when that one's window
    // could not be opened (with no bar, nothing could be chosen but by pointing).
    let Some((home, window)) = opened_home else {
        selection.update(cx, |selection, _| selection.up.store(false, Ordering::Release));
        return None;
    };
    let home_scale = selection.update(cx, |selection, cx| {
        if selection.home != home {
            eprintln!("selection {id}: the bar is on monitor {home}: that of the pointer did not open");
            selection.home = home;
            let on = selection.home_monitor().clone();
            let scale = on.scale as f32;
            selection.list = List::new(
                selection.session.windows(),
                on.bounds.width as f32 / scale,
                on.bounds.height as f32 / scale,
            );
            let pointer = selection.pointer;
            selection.pointer_at(pointer, cx);
            cx.notify();
        }
        selection.home_monitor().scale as f32
    });
    let _ = window.update(cx, |_, window, _| window.activate_window());
    cx.activate(true);
    eprintln!(
        "selection {id}: the windows opened {:.0} ms after the shortcut",
        asked.elapsed().as_secs_f64() * 1000.0
    );
    take_window_pictures(&selection, own, home_scale, cx);

    let shown_anyway = selection.downgrade();
    cx.spawn(async move |cx| {
        cx.background_executor().timer(SHOW_ANYWAY_AFTER).await;
        let _ = shown_anyway.update(cx, |selection, _| {
            if !selection.finished {
                for &handle in &selection.handles {
                    platform::set_cloaked(handle, false);
                }
                cursor::refresh();
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
        selection.up.store(false, Ordering::Release);
        for &handle in &selection.handles {
            hwnd::hide(handle);
        }
    });
}

pub fn close(selection: &Entity<Selection>, cx: &mut App) {
    let overlays = selection.update(cx, |selection, _| {
        selection.finished = true;
        selection.up.store(false, Ordering::Release);
        selection.handles.clear();
        std::mem::take(&mut selection.overlays)
    });
    // (The system's pointer is back for whatever is under it now.)
    cursor::set_hidden(false);
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
        Some(choice.and_then(|choice| selection.cut_of(&choice)))
    });
    let Some(cut) = chosen else {
        return;
    };
    close(selection, cx);
    capture::ended(selection, cx);
    // (A choice all off the screen: the screen just comes down.)
    if let Some((source, area)) = cut {
        capture::deliver(source, area, cx);
    }
}

pub fn set_mode(selection: &Entity<Selection>, mode: Mode, cx: &mut App) {
    let choice = selection.update(cx, |selection, cx| {
        if selection.finished {
            return None;
        }
        selection.pressed = false;
        if let Some(choice) = selection.session.set_mode(mode) {
            return Some(choice);
        }
        // The pointer has not moved, but what it points at means something else now.
        if let Ok(pointer) = platform::desktop().pointer() {
            selection.session.pointer_moved(pointer);
        }
        selection.bar.mode_changed();
        selection.bar_changed(cx);
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
pub struct Overlay {
    selection: Entity<Selection>,
    monitor: usize,
    /// Its monitor, as the system has it (physical pixels).
    on: Monitor,
    still: Arc<RenderImage>,
    focus: FocusHandle,
    drawn: bool,
    _observing: Subscription,
}

/// The shade over what would not be taken.
const SHADE: u32 = 0x0000_0073; // rgb(0 0 0 / 45%)

impl Overlay {
    fn new(
        selection: Entity<Selection>,
        monitor: usize,
        on: Monitor,
        still: Arc<RenderImage>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let observing = cx.observe(&selection, |_, _, cx| cx.notify());
        // Every window hears keys while it is the active one: at first the one the pointer started
        // on, then whichever was clicked (as every page of the Tauri version listened).
        let focus = cx.focus_handle();
        focus.focus(window, cx);
        Self {
            selection,
            monitor,
            on,
            still,
            focus,
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

    /// Whether this is over the bar, which keeps what happens on it to itself.
    fn over_bar(&self, position: gpui::Point<Pixels>, cx: &App) -> bool {
        let selection = self.selection.read(cx);
        selection.home == self.monitor
            && selection
                .bar
                .drawn
                .get()
                .is_some_and(|drawn| drawn.seen.contains(&position))
    }

    fn press(&mut self, press: Press, position: gpui::Point<Pixels>, window: &Window, cx: &mut Context<Self>) {
        let at = self.screen_at(position, window);
        let over_bar = self.over_bar(position, cx);
        let choice = self.selection.update(cx, |selection, cx| {
            if selection.finished {
                return None;
            }
            // (What happens on the bar is the bar's, unless it is a press of the screen held on.)
            let screens = selection.pressed || !over_bar;
            let choice = match press {
                Press::Down if screens => {
                    selection.pressed = true;
                    selection.session.pointer_down(at);
                    None
                }
                Press::Moved if screens => {
                    selection.session.pointer_moved(at);
                    None
                }
                Press::Up if selection.pressed => {
                    selection.pressed = false;
                    selection.session.pointer_up(at)
                }
                _ => None,
            };
            if choice.is_none() {
                selection.pointer_at(at, cx);
                // (Every move: the + follows the pointer.)
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
        let entity = self.selection.clone();
        let selection = self.selection.read(cx);
        let highlight = selection.session.highlight();
        let mode = selection.session.mode();
        let home = selection.home == self.monitor;

        if !self.drawn {
            self.drawn = true;
            // Seen once the still is on screen: never an empty window first. The first frame is
            // drawn as the window opens and put on screen at the next; what is asked for at the
            // next comes before that is done, so the window is shown at the one after.
            let (asked, id, monitor) = (selection.asked, selection.id, self.monitor);
            window.on_next_frame(move |window, _| {
                window.on_next_frame(move |window, _| {
                    if let Some(handle) = hwnd::of(window) {
                        platform::set_cloaked(handle, false);
                    }
                    cursor::refresh();
                    eprintln!(
                        "selection {id}: monitor {monitor} shown {:.0} ms after the shortcut",
                        asked.elapsed().as_secs_f64() * 1000.0
                    );
                });
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
            .child(img(self.still.clone()).absolute().top_0().left_0().size_full())
            .track_focus(&self.focus)
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, _, cx| this.on_key(event, cx)));

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
                root = root.child(frame.border_color(rgba(bar::CURRENT)));
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

        if home {
            let windows = selection.session.windows();
            let listed = mode == Mode::Window;
            if selection.bar.sliding() || (listed && selection.list.moving(windows.len())) {
                window.request_animation_frame();
            }
            if listed {
                let accent = perora_ui::Theme::get(cx).accent;
                root = root.child(window_list::element(
                    &selection.list,
                    windows,
                    &entity,
                    &selection.language,
                    accent,
                ));
            }
            root = root.child(bar::element(selection, &entity, mode, &selection.language));
        }

        // The + over everything, where the pointer is on this monitor: not over the bar, where the
        // system's pointer is shown. The window the pointer is over says whether the system's is.
        let here = self
            .on
            .bounds
            .contains(selection.pointer)
            .then(|| self.local(Rect::new(selection.pointer.x, selection.pointer.y, 1, 1), scale).origin);
        let crosshair = selection.crosshair;
        let drawn = home.then(|| selection.bar.drawn.clone());
        let cards = (home && mode == Mode::Window)
            .then(|| (selection.list.room.clone(), selection.list.cards.clone()));
        root = root.child(
            canvas(
                |_, _, _| {},
                move |_, _, window, _| {
                    let Some(at) = here else {
                        return;
                    };
                    let over_bar = drawn
                        .as_ref()
                        .and_then(|drawn| drawn.get())
                        .is_some_and(|drawn| drawn.seen.contains(&at));
                    let over_card = cards.as_ref().is_some_and(|(room, cards)| {
                        room.get().is_some_and(|room| room.contains(&at))
                            && cards.borrow().iter().flatten().any(|card| card.contains(&at))
                    });
                    let over_own = over_bar || over_card;
                    cursor::set_hidden(crosshair && !over_own);
                    if crosshair && !over_own {
                        paint_crosshair(at, window);
                    }
                },
            )
            .absolute()
            .size_full(),
        );

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

/// Takes the picture of each window, front to back, while the selection screen is up, until the
/// budget is used. Beside it, the small picture of every window is made for the list of them, at
/// the size of its place there. Both stop when the screen is gone. Perora's own (`own`) are hidden by
/// then: of those, what the capture has of them is all there is.
fn take_window_pictures(selection: &Entity<Selection>, own: Vec<WindowId>, scale: f32, cx: &mut App) {
    let (id, still, windows, sizes, pictures, up) = {
        let selection = selection.read(cx);
        let windows: Vec<(WindowId, Rect)> = selection
            .session
            .windows()
            .iter()
            .map(|window| (window.id, window.bounds))
            .collect();
        (
            selection.id,
            selection.still.clone(),
            windows,
            selection.list.sizes(selection.session.windows(), scale),
            selection.pictures.clone(),
            selection.up.clone(),
        )
    };
    let started = Instant::now();

    // The small pictures, on a thread of their own as the pictures come: those are what a choice
    // needs, and they are not to wait for these. Of every window: of its own picture, or when it
    // gave none, of what is seen of it on the screen.
    let (made, to_make) = std::sync::mpsc::channel::<(usize, Rect, Option<Arc<Frame>>)>();
    let (ready, arrived) = async_channel::unbounded::<(usize, Arc<RenderImage>)>();
    std::thread::spawn({
        let up = up.clone();
        move || {
            for (index, bounds, picture) in to_make {
                if !up.load(Ordering::Acquire) {
                    return;
                }
                let (width, height) = sizes[index];
                let small = match picture {
                    Some(picture) => Some(picture.scaled_to_fit(width, height)),
                    None => still
                        .crop(bounds)
                        .map(|seen| seen.scaled_to_fit(width, height)),
                };
                if let Some(small) = small.and_then(drawable)
                    && ready.send_blocking((index, small)).is_err()
                {
                    return;
                }
            }
            eprintln!(
                "selection {id}: the list of windows ready after {:.0?}",
                started.elapsed()
            );
        }
    });

    std::thread::spawn(move || {
        let desktop = platform::desktop();
        let (mut used, mut taken) = (0usize, 0usize);
        for (index, &(window, bounds)) in windows.iter().enumerate() {
            if !up.load(Ordering::Acquire) {
                return;
            }
            // Out of room: the rest are cut out of the capture of the screen.
            let picture = if used > WINDOW_PICTURES_BUDGET || own.contains(&window) {
                None
            } else {
                match desktop.capture_window(window) {
                    Ok(picture) => {
                        used += picture.rgba.len();
                        (used <= WINDOW_PICTURES_BUDGET).then(|| Arc::new(picture))
                    }
                    Err(error) => {
                        eprintln!("selection {id}: no picture of window {window:?}: {error}");
                        None
                    }
                }
            };
            if let Some(picture) = &picture {
                pictures
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner())
                    .insert(window, picture.clone());
                taken += 1;
            }
            // The other thread is gone only when the screen is.
            if made.send((index, bounds, picture)).is_err() {
                return;
            }
        }
        eprintln!(
            "selection {id}: pictures of {taken} of {} windows in {:.0?}",
            windows.len(),
            started.elapsed()
        );
    });

    let list = selection.downgrade();
    cx.spawn(async move |cx| {
        while let Ok((index, picture)) = arrived.recv().await {
            let shown = list.update(cx, |selection, cx| {
                selection.list.pictures.insert(index, (picture, Instant::now()));
                cx.notify();
            });
            if shown.is_err() {
                return;
            }
        }
        let _ = list.update(cx, |selection, cx| {
            selection.list.all_made = true;
            cx.notify();
        });
    })
    .detach();
}

/// A + on the pointer's pixel (`at`, its corner): two lines 31 px long that cross on it, white in a
/// black edge, seen on anything. A whole +, the pixel included (owner, 2026-11-02: 「真ん中開けずに
/// ちゃんとした + であってほしい」).
fn paint_crosshair(at: gpui::Point<Pixels>, window: &mut Window) {
    const ARM: f32 = 15.0;
    let black: Hsla = gpui::black().opacity(0.7);
    let white: Hsla = gpui::white();
    let line = |x: f32, y: f32, width: f32, height: f32| {
        Bounds::new(point(at.x + px(x), at.y + px(y)), size(px(width), px(height)))
    };
    // The edge: down, then across without the part where they cross (it is drawn once, not
    // darker).
    window.paint_quad(fill(line(-1.0, -ARM, 3.0, 2.0 * ARM + 1.0), black));
    window.paint_quad(fill(line(-ARM, -1.0, ARM - 1.0, 3.0), black));
    window.paint_quad(fill(line(2.0, -1.0, ARM - 1.0, 3.0), black));
    window.paint_quad(fill(line(0.0, -ARM, 1.0, 2.0 * ARM + 1.0), white));
    window.paint_quad(fill(line(-ARM, 0.0, 2.0 * ARM + 1.0, 1.0), white));
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
