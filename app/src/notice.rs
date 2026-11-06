//! What Perora has to say when no window of its own is the place to say it: that a capture was copied
//! or saved, what went wrong. Said in a small window of Perora's own, in a corner of the primary
//! monitor that the user chooses, rather than through the system's notifications: those queue up,
//! each waiting for the one before it to leave (owner: 「専用の通知ウィンドウ作りたい」).
//!
//! A notice is never in a capture, nor over the screen a capture is selected on: it waits until
//! that is over, and one on screen is put away as a capture starts.
//!
//! There is one notice at a time. A new one takes the place of the one on screen at once, and stays
//! for as long as it is to, whatever was left of the time of the one before; a notice leaves on its
//! own when its time is over, or when it is clicked. The window never takes the keyboard.
//!
//! The window is made when a notice appears and closed when it leaves: while it is open, the GPU is
//! held. (P3: the slide into its corner, and keeping clear of the small preview.)

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use gpui::{
    App, AppContext, Context, CursorStyle, Global, InteractiveElement, IntoElement, ParentElement,
    Render, RenderImage, StatefulInteractiveElement, Styled, Window, WindowBackgroundAppearance,
    WindowBounds, WindowHandle, WindowKind, WindowOptions, div, img, px,
};
use perora_core::geometry::Rect;
use perora_core::notice::Sound;
use perora_core::platform::{self, ScreenCapture};
use perora_ui::Theme;

use crate::state::Perora;
use crate::{capture, display, gpu, hwnd, main_window};

/// The window, in logical pixels: as wide as the small preview, which it lines up with.
const SIZE: (f32, f32) = (360.0, 76.0);
/// How far from the edges of the work area (logical pixels).
const MARGIN: f32 = 16.0;
/// How often a notice that waits for a capture to be over looks whether it is.
const WAITS: Duration = Duration::from_millis(50);

/// How long a notice is on screen, which depends on what it says.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lasting {
    /// Something went well: read at a glance.
    Brief,
    /// Something went wrong: there is a reason to read.
    Long,
    /// Something is under way, and another notice will say how it ended: until that one comes (or,
    /// should it never, until this is plainly stale).
    #[allow(dead_code)]
    UntilTheNext,
}

impl Lasting {
    /// What says how something ended stays as long as the user chose (`seconds`), whichever way
    /// it ended; with 0, for good: `None`, and it leaves when it is clicked or another is said.
    fn time(self, seconds: f64) -> Option<Duration> {
        match self {
            Lasting::Brief | Lasting::Long => {
                (seconds > 0.0).then(|| Duration::from_secs_f64(seconds))
            }
            Lasting::UntilTheNext => Some(Duration::from_secs(90)),
        }
    }
}

/// What a click on a notice does, besides sending it away.
#[derive(Debug, Clone)]
pub enum Click {
    /// Shows the file the notice is about in its folder.
    Reveal(PathBuf),
    /// Opens the main window: what the notice is about is shown there.
    #[allow(dead_code)]
    OpenMain,
}

#[derive(Default)]
struct Notices {
    /// What is on screen, or about to be.
    message: Option<String>,
    /// What a click on it does, when it does more than send it away.
    click: Option<Click>,
    /// Which notice that is, counted from the first: what is to happen to one notice later (it
    /// leaves) does not happen to the one that took its place.
    turn: u64,
    window: Option<WindowHandle<Notice>>,
    icon: Option<Arc<RenderImage>>,
}

impl Global for Notices {}

/// Says something, in place of whatever is being said.
pub fn say(message: String, lasting: Lasting, click: Option<Click>, cx: &mut App) {
    let notices = cx.default_global::<Notices>();
    notices.turn += 1;
    notices.message = Some(message);
    notices.click = click;
    let turn = notices.turn;
    let window = notices.window;
    // (On screen already: it says this now.)
    if let Some(window) = window {
        let _ = window.update(cx, |_, _, cx| cx.notify());
    }

    cx.spawn(async move |cx| {
        // Not while the screen is being captured or a capture is being selected: the notice would
        // be in the picture, or in the way. It is said when that is over.
        loop {
            let waits = cx.update(|cx| still(turn, cx) && capture::busy(cx));
            if !waits {
                break;
            }
            cx.background_executor().timer(WAITS).await;
        }
        if !cx.update(|cx| still(turn, cx)) {
            return;
        }
        let seconds = cx.update(|cx| {
            show(cx);
            // Heard as it appears. Not what only says that something is under way: what it ends
            // in is said a moment later, and is heard.
            if lasting != Lasting::UntilTheNext {
                sound(cx);
            }
            Perora::get(cx).settings().notice_seconds
        });
        // (As long as the settings say when it appears: not changed under it afterwards.)
        if let Some(time) = lasting.time(seconds) {
            cx.background_executor().timer(time).await;
            cx.update(|cx| put_away_if(Some(turn), cx));
        }
    })
    .detach();
}

/// A notice of a file: a click on it shows the file in its folder.
pub fn say_of_file(message: String, lasting: Lasting, file: PathBuf, cx: &mut App) {
    say(message, lasting, Some(Click::Reveal(file)), cx);
}

fn still(turn: u64, cx: &mut App) -> bool {
    cx.default_global::<Notices>().turn == turn
}

/// Takes the notice off the screen, whichever it is. Returns whether there was one: a window that
/// was just hidden is on screen for one more frame, which matters to whoever captures the screen
/// next.
pub fn put_away(cx: &mut App) -> bool {
    put_away_if(None, cx)
}

/// The one of that turn, or whichever there is.
fn put_away_if(turn: Option<u64>, cx: &mut App) -> bool {
    taken_away(
        |notices| turn.is_none_or(|turn| turn == notices.turn && notices.message.is_some()),
        cx,
    )
    .0
}

/// Takes the notice off the screen when it is `the_one`, and says whether there was one, with what
/// a click on it does: taken in the same breath, so that it is never what another notice does.
fn taken_away(the_one: impl FnOnce(&Notices) -> bool, cx: &mut App) -> (bool, Option<Click>) {
    let notices = cx.default_global::<Notices>();
    if !the_one(notices) {
        return (false, None);
    }
    // Whatever was still to happen to it does not.
    notices.turn += 1;
    let click = notices.click.take();
    let was = notices.message.take().is_some();
    if let Some(window) = notices.window.take() {
        let _ = window.update(cx, |_, window, _| {
            // Out of sight at once: closing takes a turn of the message loop.
            if let Some(handle) = hwnd::of(window) {
                hwnd::hide(handle);
            }
            window.remove_window();
        });
    }
    (was, click)
}

/// The notice was clicked: read, and in the way. One that is about a file shows that file in its
/// folder as it goes. The click says which notice the window showed: when another has taken its
/// place meanwhile, that one stays, unread, and nothing is shown.
fn dismiss(turn: u64, cx: &mut App) {
    let (_, click) = taken_away(|notices| notices.turn == turn, cx);
    match click {
        Some(Click::Reveal(file)) => {
            std::thread::spawn(move || {
                if let Err(error) = platform::reveal_in_folder(&file) {
                    eprintln!("the folder of what was saved could not be shown: {error}");
                }
            });
        }
        Some(Click::OpenMain) => main_window::open(cx),
        None => {}
    }
}

/// On screen, in the corner the user chose of the primary monitor's work area (clear of the
/// taskbar).
fn show(cx: &mut App) {
    if cx.default_global::<Notices>().window.is_some() {
        return;
    }
    let corner = Perora::get(cx).settings().notice_corner;
    let Some(monitor) = platform::screen_capture().layout().ok().and_then(|layout| {
        layout
            .primary()
            .or(layout.monitors.first())
            .cloned()
    }) else {
        return;
    };
    let Some(screen) = display::of(&monitor, cx) else {
        return;
    };
    let scale = monitor.scale;
    let (width, height) = (
        (f64::from(SIZE.0) * scale).round() as i32,
        (f64::from(SIZE.1) * scale).round() as i32,
    );
    let margin = (f64::from(MARGIN) * scale).round() as i32;
    let area = monitor.work_area;
    let place = Rect::new(
        if corner.is_right() {
            area.right() - width - margin
        } else {
            area.x + margin
        },
        if corner.is_bottom() {
            area.bottom() - height - margin
        } else {
            area.y + margin
        },
        width as u32,
        height as u32,
    );

    gpu::attach(cx);
    let options = WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(display::bounds_on(&monitor, place))),
        titlebar: None,
        focus: false,
        show: true,
        kind: WindowKind::PopUp,
        is_movable: false,
        is_resizable: false,
        is_minimizable: false,
        display_id: Some(screen.id()),
        window_background: WindowBackgroundAppearance::Opaque,
        ..Default::default()
    };
    let opened = cx.open_window(options, |window, cx| {
        if let Some(handle) = hwnd::of(window) {
            platform::without_transitions(handle);
            // Never takes the keyboard: not when it appears, not when it is clicked.
            hwnd::never_active(handle);
        }
        cx.new(|_| Notice { turn: 0 })
    });
    match opened {
        Ok(window) => cx.default_global::<Notices>().window = Some(window),
        Err(error) => eprintln!("the notice could not be shown: {error}"),
    }
}

fn sound(cx: &mut App) {
    let settings = Perora::get(cx).settings();
    let file = match settings.notice_sound {
        Sound::None => None,
        Sound::System(name) => platform::system_sound(&name),
        Sound::Custom => settings.notice_sound_file,
    };
    if let Some(file) = file {
        platform::play_sound(&file);
    }
}

/// Perora's icon, at the size the notice shows it (made once).
fn icon(cx: &mut App) -> Option<Arc<RenderImage>> {
    let notices = cx.default_global::<Notices>();
    if notices.icon.is_none() {
        let file = include_bytes!("../../src-tauri/icons/128x128.png");
        notices.icon = platform::Frame::from_png(file, 1 << 20).and_then(crate::selection::drawable);
    }
    notices.icon.clone()
}

struct Notice {
    /// The turn of what it shows: a click says which notice it was on.
    turn: u64,
}

impl Render for Notice {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let icon = icon(cx);
        let notices = cx.default_global::<Notices>();
        self.turn = notices.turn;
        let (message, opens) = (notices.message.clone(), notices.click.is_some());
        let theme = Theme::get(cx);
        let turn = self.turn;
        div()
            .id("notice")
            .relative()
            .size_full()
            .flex()
            .items_center()
            .gap(px(12.0))
            .pt(px(10.0))
            .pb(px(10.0))
            .pr(px(14.0))
            .pl(px(13.0))
            .bg(theme.surface)
            .text_color(theme.text)
            .font_family(theme.font.clone())
            .text_size(theme.text_size_small)
            .cursor(if opens {
                CursorStyle::PointingHand
            } else {
                CursorStyle::Arrow
            })
            .on_click(move |_, _, cx| cx.defer(move |cx| dismiss(turn, cx)))
            // The edge, and down the left of it the accent colour.
            .child(div().absolute().inset_0().border_1().border_color(theme.border))
            .child(div().absolute().left_0().top_0().bottom_0().w(px(3.0)).bg(theme.accent))
            .children(message.map(|message| {
                div()
                    .flex()
                    .items_center()
                    .gap(px(12.0))
                    .children(icon.map(|icon| img(icon).flex_none().size(px(32.0))))
                    .child(div().flex_1().line_clamp(3).child(message))
            }))
    }
}
