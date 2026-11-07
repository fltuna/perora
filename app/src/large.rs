//! The large window: the capture to draw on (the editor), with what it leaves as at the top right
//! and what is said of it under the picture. A click on the small preview's picture brings the
//! capture here, or it comes here at once (`afterCapture: edit`). Closing it throws the capture away,
//! as the bin does.
//!
//! The window is opened as the capture comes and closed as it goes (the GPU is held while it is
//! open), and only hidden while a capture is taken, so that what was drawn stays.
//!
//! Asked for from the main window with no capture to show, it opens empty and waits for an image
//! (`await_image`): dropped on it, chosen, or from the clipboard (`opening`).
//!
//! P4 and P5 of docs/gpui-rewrite.md. Not yet: uploading, the tour's i (P6).

use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant};

use gpui::prelude::FluentBuilder;
use gpui::{
    App, AppContext, Bounds, Context, CursorStyle, Entity, Focusable, Global, InteractiveElement,
    IntoElement, ParentElement, Render, SharedString, StatefulInteractiveElement, Styled,
    Subscription, TitlebarOptions, WeakEntity, Window, WindowBounds, WindowHandle, WindowOptions,
    MouseButton, MouseDownEvent, deferred, div, point, px, size,
};
use perora_core::output::{FITS, Format};
use perora_core::settings::Leaves;
use perora_core::edit::Edit;
use perora_core::language::Language;
use perora_core::platform::{self, Frame, WindowId};
use perora_editor::{Capture, Editor, Event, Leave, Open, Picture};
use perora_ui::{Theme, Tip};

use crate::state::Perora;
use crate::{account, gpu, hwnd, preview};

/// The size the Tauri version opens at, and the smallest it may be (logical pixels, inside).
const SIZE: (f32, f32) = (1200.0, 800.0);
/// How often what is said of the clipboard is looked at anew.
const LOOKS_AT_CLIPBOARD: Duration = Duration::from_millis(500);
/// A window hidden for a capture is out of sight (cloaked) a little longer than it is hidden, so that
/// the system's fade does not end up in the picture.
const UNCLOAKED_AFTER: Duration = Duration::from_millis(400);

#[derive(Default)]
struct Large {
    window: Option<WindowHandle<Editor>>,
    /// Which showing is the last: a picture made for an earlier one is not shown.
    showing: u64,
    /// The window was asked for with no capture in it, and waits for an image: it comes back so
    /// after a capture that came to nothing.
    awaiting: bool,
    /// Waiting so, it was put out of the way of a capture while on screen: whether it was the
    /// window in front then (it comes back as it was).
    put_aside: Option<bool>,
    /// What the window puts beside the editor: what is said in the user's language.
    actions: Option<Entity<Actions>>,
    status: Option<Entity<Status>>,
    /// The tour of the window was seen since Perora started (before the settings say so).
    toured: bool,
    _subscription: Option<Subscription>,
}

impl Global for Large {}

/// The capture in the large window, drawn on anew when it is another than the one there; the window
/// opened, or brought back and to the front. The picture is made beside the window first (a large
/// capture takes a while), when it is not given.
pub fn show(id: u64, frame: Arc<Frame>, picture: Option<Picture>, cx: &mut App) {
    let large = cx.default_global::<Large>();
    large.showing += 1;
    let showing = large.showing;
    if let Some(window) = large.window {
        let shows = window
            .update(cx, |editor, _, _| editor.capture_id() == Some(id))
            .unwrap_or(false);
        if shows {
            let _ = window.update(cx, |_, window, _| {
                if let Some(handle) = hwnd::of(window) {
                    platform::set_cloaked(handle, false);
                    hwnd::show(handle);
                }
                window.activate_window();
            });
            return;
        }
    }
    let making = match picture {
        Some(picture) => cx.background_executor().spawn(async move { Some(picture) }),
        None => cx
            .background_executor()
            .spawn(async move { Picture::new(&frame) }),
    };
    cx.spawn(async move |cx| {
        let Some(picture) = making.await else {
            eprintln!("large {id}: no picture could be made of the capture");
            return;
        };
        cx.update(|cx| {
            if cx.default_global::<Large>().showing != showing {
                return;
            }
            present(id, picture, cx);
        });
    })
    .detach();
}

fn present(id: u64, picture: Picture, cx: &mut App) {
    let capture = Capture {
        id,
        picture: Rc::new(picture),
    };
    let large = cx.default_global::<Large>();
    large.awaiting = false;
    large.put_aside = None;
    open(Some(capture), cx);
}

/// The window with no capture in it, waiting for an image to be dropped on it, chosen or taken from
/// the clipboard: opened, or brought back and to the front.
pub fn await_image(cx: &mut App) {
    let large = cx.default_global::<Large>();
    large.awaiting = true;
    large.put_aside = None;
    if let Some(window) = cx.default_global::<Large>().window {
        let _ = window.update(cx, |editor, window, cx| {
            if let Some(handle) = hwnd::of(window) {
                platform::set_cloaked(handle, false);
                hwnd::show(handle);
            }
            window.activate_window();
            window.focus(&editor.focus_handle(cx), cx);
        });
        return;
    }
    open(None, cx);
}

/// Whether the window waits for an image (on screen, or out of the way of a capture).
pub fn awaiting(cx: &mut App) -> bool {
    cx.default_global::<Large>().awaiting
}

/// After a capture that came to nothing: the window that waits for an image is back where it was
/// put out of the way, in front only if it was in front (not over a game the capture was taken
/// from). Not when it was not on screen.
pub fn await_again(cx: &mut App) {
    let large = cx.default_global::<Large>();
    let Some(in_front) = large.put_aside.take() else {
        return;
    };
    if !large.awaiting {
        return;
    }
    let Some(window) = large.window else {
        return;
    };
    let _ = window.update(cx, |editor, window, cx| {
        let Some(handle) = hwnd::of(window) else {
            return;
        };
        platform::set_cloaked(handle, false);
        if in_front {
            hwnd::show(handle);
            window.activate_window();
            window.focus(&editor.focus_handle(cx), cx);
        } else {
            hwnd::show_behind(handle);
        }
    });
}

/// The language changed: what the window says, in the new one.
pub fn relanguage(cx: &mut App) {
    let language = Perora::get(cx).language();
    let large = cx.default_global::<Large>();
    let (window, actions, status) = (large.window, large.actions.clone(), large.status.clone());
    if let Some(actions) = actions {
        actions.update(cx, |actions, cx| {
            actions.language = language.clone();
            cx.notify();
        });
    }
    if let Some(status) = status {
        status.update(cx, |status, cx| {
            status.language = language.clone();
            cx.notify();
        });
    }
    if let Some(window) = window {
        let _ = window.update(cx, |editor, _, cx| editor.set_language(language, cx));
    }
}

/// The window with the capture (or with none, waiting for an image), opened when it is not, and in
/// front.
fn open(capture: Option<Capture>, cx: &mut App) {
    // (Which way to make a part unreadable the editor starts with, as the settings say.)
    let hider = Perora::get(cx).settings().default_hider;
    let seen = tour_seen(cx);
    if let Some(window) = cx.default_global::<Large>().window {
        let _ = window.update(cx, move |editor, window, cx| {
            editor.set_default_hider(hider);
            editor.set_tour_seen(seen);
            if let Some(capture) = capture {
                editor.show(capture, cx);
                editor.offer_tour(cx);
            }
            if let Some(handle) = hwnd::of(window) {
                // (Faded in as it comes back; on screen already, it stays as it is.)
                if !hwnd::on_screen(handle) {
                    fade_in(handle, cx);
                }
                platform::set_cloaked(handle, false);
                hwnd::show(handle);
            }
            window.activate_window();
            window.focus(&editor.focus_handle(cx), cx);
        });
        return;
    }
    gpu::attach(cx);
    crate::main_window::apply_theme(cx);
    let language = Perora::get(cx).language();
    let bounds = Bounds::centered(None, size(px(SIZE.0), px(SIZE.1)), cx);
    let options = WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(bounds)),
        titlebar: Some(TitlebarOptions {
            title: Some("Perora".into()),
            ..Default::default()
        }),
        window_min_size: Some(size(px(SIZE.0), px(SIZE.1))),
        ..Default::default()
    };
    let mut parts = None;
    let opened = cx.open_window(options, |window, cx| {
        // Closed (its button, Alt+F4): thrown away, as the bin does. (Waiting for an image: closed.)
        window.on_window_should_close(cx, |_, cx| {
            cx.defer(|cx| {
                if shows_nothing(cx) {
                    close(cx);
                } else {
                    preview::discard_by_user(None, cx);
                }
            });
            false
        });
        let editor = cx.new(|cx| {
            let mut editor = Editor::new(language.clone(), window, cx);
            editor.set_default_hider(hider);
            editor.set_tour_seen(seen);
            if let Some(capture) = capture {
                editor.show(capture, cx);
                editor.offer_tour(cx);
            }
            editor
        });
        let actions = cx.new(|cx| Actions::new(language.clone(), &editor, cx));
        let status = cx.new(|cx| Status::new(language.clone(), &editor, cx));
        parts = Some((actions.clone(), status.clone()));
        editor.update(cx, |editor, _| {
            editor.top_right = Some(actions.into());
            editor.below = Some(status.into());
        });
        window.focus(&editor.focus_handle(cx), cx);
        if let Some(handle) = hwnd::of(window) {
            fade_in(handle, cx);
        }
        // In front: opened from a click on the small window (which never takes the keyboard), Perora
        // is not the application in front, and the system opens a new window behind it unless asked
        // to bring it forward (GPUI's activation sees to that).
        window.activate_window();
        editor
    });
    match opened {
        Ok(window) => {
            let subscription = window
                .update(cx, |_, _, cx| cx.entity())
                .ok()
                .map(|editor| {
                    cx.subscribe(&editor, |editor, event, cx| asked(editor, event.clone(), cx))
                });
            let large = cx.default_global::<Large>();
            large.window = Some(window);
            large._subscription = subscription;
            if let Some((actions, status)) = parts {
                large.actions = Some(actions);
                large.status = Some(status);
            }
        }
        Err(error) => eprintln!("the large window could not be opened: {error}"),
    }
}

/// How the window comes in: faded in (0.16 s, in 12 steps), as the Tauri version's did. Not for a
/// user who has the system move nothing.
const FADE: Duration = Duration::from_millis(160);
const FADE_STEPS: u32 = 12;

fn fade_in(handle: WindowId, cx: &mut App) {
    if !platform::animations_enabled() {
        return;
    }
    platform::set_opacity(handle, Some(0));
    cx.spawn(async move |cx| {
        for step in 1..=FADE_STEPS {
            cx.background_executor().timer(FADE / FADE_STEPS).await;
            platform::set_opacity(handle, Some((255 * step / FADE_STEPS) as u8));
        }
        platform::set_opacity(handle, None);
    })
    .detach();
}

/// What the editor asks of the window.
fn asked(editor: Entity<Editor>, event: Event, cx: &mut App) {
    // (An image to open: whether there is one in the window now or not.)
    match event {
        Event::Open(Open::File(file)) => return crate::opening::open_file(file, cx),
        Event::Open(Open::Choose) => return crate::opening::choose_file(cx),
        Event::Open(Open::Clipboard) => return crate::opening::open_clipboard(cx),
        _ => {}
    }
    let Some(id) = editor.read(cx).capture_id() else {
        // Waiting for an image: Esc and its button close it.
        if event == Event::Discard {
            close(cx);
        }
        return;
    };
    match event {
        Event::Changed => changed(cx),
        Event::Leave(Leave::Save) => preview::save(id, cx),
        // (Ctrl+Enter: uploading when it is on offer, saving otherwise.)
        Event::Leave(Leave::Upload) if account::uploads(cx) => preview::upload(id, cx),
        Event::Leave(Leave::Upload) => preview::save(id, cx),
        Event::Discard => preview::discard_by_user(Some(id), cx),
        Event::Toured => saw_tour(cx),
        Event::Open(_) => {}
    }
}

/// Whether the tour of the window was seen: the settings say so, or it was, since Perora started.
fn tour_seen(cx: &mut App) -> bool {
    cx.default_global::<Large>().toured
        || Perora::get(cx)
            .settings()
            .tours_seen
            .contains(&perora_core::settings::Tour::Editor)
}

/// The tour of the window was seen (to the end, or put away): it does not come on its own again.
fn saw_tour(cx: &mut App) {
    let large = cx.default_global::<Large>();
    if large.toured {
        return;
    }
    large.toured = true;
    let perora = Perora::get(cx);
    if let Some(path) = perora.settings_path.clone() {
        perora_core::settings::tour_seen(&path, &perora.settings, "editor");
    }
}

/// Whether the window is there with no capture in it (waiting for an image).
fn shows_nothing(cx: &mut App) -> bool {
    let Some(window) = cx.default_global::<Large>().window else {
        return false;
    };
    window
        .read_with(cx, |editor, _| editor.capture_id().is_none())
        .unwrap_or(false)
}

/// The window, for the system's choice of a file to be over (it cannot be used meanwhile).
pub fn handle(cx: &mut App) -> Option<WindowId> {
    let window = cx.default_global::<Large>().window?;
    window
        .update(cx, |_, window, _| hwnd::of(window))
        .ok()
        .flatten()
}

/// Closes the window (its capture is gone, or it waited for an image and is given up).
pub fn close(cx: &mut App) {
    let large = cx.default_global::<Large>();
    large.showing += 1;
    large.awaiting = false;
    large.put_aside = None;
    large.actions = None;
    large.status = None;
    large._subscription = None;
    let Some(window) = large.window.take() else {
        return;
    };
    let _ = window.update(cx, |_, window, _| {
        if let Some(handle) = hwnd::of(window) {
            hwnd::hide(handle);
        }
        window.remove_window();
    });
}

/// Out of the way of a capture: out of sight at once (cloaked, then hidden, so that the system's
/// fade is not in the picture), and back with `show`. Whether it was on screen.
pub fn hide(cx: &mut App) -> bool {
    let large = cx.default_global::<Large>();
    large.showing += 1;
    let Some(window) = large.window else {
        return false;
    };
    let handle = window
        .update(cx, |editor, window, cx| {
            // (Its button let go while it is hidden, nobody would hear it.)
            editor.settle(cx);
            // (Out of sight while it is on, the tour is over, and not seen.)
            editor.end_tour(false, cx);
            hwnd::of(window)
        })
        .ok()
        .flatten();
    let Some(handle) = handle.filter(|&handle| hwnd::on_screen(handle)) else {
        return false;
    };
    let large = cx.default_global::<Large>();
    if large.awaiting {
        large.put_aside = Some(hwnd::in_front(handle));
    }
    platform::set_cloaked(handle, true);
    hwnd::hide(handle);
    cx.spawn(async move |cx| {
        cx.background_executor().timer(UNCLOAKED_AFTER).await;
        cx.update(|_| {
            if !hwnd::on_screen(handle) {
                platform::set_cloaked(handle, false);
            }
        });
    })
    .detach();
    true
}

/// The window, when it is on screen: Perora's own, in the picture when the user wants that.
pub fn on_screen_now(cx: &mut App) -> Option<WindowId> {
    let window = cx.default_global::<Large>().window?;
    window
        .update(cx, |_, window, _| {
            hwnd::of(window).filter(|&handle| hwnd::on_screen(handle))
        })
        .ok()
        .flatten()
}

/// The capture in the window and what is drawn on it, when it is that one.
pub fn drawing_of(id: u64, cx: &mut App) -> Option<Vec<Edit>> {
    let window = cx.default_global::<Large>().window?;
    window
        .read_with(cx, |editor, _| {
            (editor.capture_id() == Some(id)).then(|| editor.edits().to_vec())
        })
        .ok()
        .flatten()
}

/// The capture in the window, when something is drawn on it: what a new capture or an image opened
/// would take the place of.
pub fn drawn_on(cx: &mut App) -> Option<u64> {
    let window = cx.default_global::<Large>().window?;
    window
        .read_with(cx, |editor, _| {
            editor
                .capture_id()
                .filter(|_| !editor.edits().is_empty())
        })
        .ok()
        .flatten()
}

/// When something is drawn in the window: asks whether it may be given up for what comes next
/// (`said` says what that is; going ahead, `go`, is the first answer). None when nothing is drawn
/// (or there is no window).
pub fn ask_over_drawing(
    said: &str,
    go: String,
    cx: &mut App,
) -> Option<impl std::future::Future<Output = Option<usize>> + use<>> {
    let window = cx.default_global::<Large>().window?;
    let language = Perora::get(cx).language();
    drawn_on(cx)?;
    window
        .update(cx, |_, window, cx| {
            window.prompt(
                gpui::PromptLevel::Warning,
                said,
                None,
                &[
                    gpui::PromptButton::ok(go),
                    gpui::PromptButton::cancel(language.keep_drawing()),
                ],
                cx,
            )
        })
        .ok()
        .map(|answer| async move { answer.await.ok() })
}

/// Before the capture leaves from the window: what is being written in it is written. False when it
/// cannot be (too large to be a picture, a drawing that holds no more): it stays being written, the
/// user is told, and nothing leaves.
pub fn done_writing(id: u64, cx: &mut App) -> bool {
    let Some(window) = cx.default_global::<Large>().window else {
        return true;
    };
    window
        .update(cx, |editor, _, cx| {
            editor.capture_id() != Some(id) || editor.done_writing(cx)
        })
        .unwrap_or(true)
}

/// Before the capture leaves: what is under way in the editor is done or given up, and nothing more
/// is drawn until it has left (or could not). `vanish`: out of sight meanwhile (saving: the window is
/// gone at once for whoever pressed; copying keeps it).
pub fn leaving(id: u64, vanish: bool, cx: &mut App) {
    let Some(window) = cx.default_global::<Large>().window else {
        return;
    };
    let _ = window.update(cx, |editor, window, cx| {
        if editor.capture_id() != Some(id) {
            return;
        }
        editor.settle(cx);
        editor.set_frozen(true, cx);
        if vanish && let Some(handle) = hwnd::of(window) {
            platform::set_cloaked(handle, true);
        }
    });
}

/// The capture did not leave (or left and stays): drawn on again, and in sight.
pub fn stays(id: u64, cx: &mut App) {
    let Some(window) = cx.default_global::<Large>().window else {
        return;
    };
    let _ = window.update(cx, |editor, window, cx| {
        if editor.capture_id() != Some(id) {
            return;
        }
        editor.set_frozen(false, cx);
        if let Some(handle) = hwnd::of(window) {
            platform::set_cloaked(handle, false);
        }
    });
}

/// Tells the window that what it says changed.
pub fn changed(cx: &mut App) {
    if let Some(window) = cx.default_global::<Large>().window {
        let _ = window.update(cx, |_, window, cx| {
            window.refresh();
            cx.notify();
        });
    }
}

/// A tooltip that says this.
fn tip(text: impl Into<SharedString>) -> impl Fn(&mut Window, &mut App) -> gpui::AnyView + 'static {
    let text = text.into();
    move |_, cx| Tip::view(text.clone(), cx)
}

/// Segoe Fluent Icons, as the Tauri version has them.
const UPLOAD: &str = "\u{E898}";
const SAVE: &str = "\u{E74E}";
const SHEETS: &str = "\u{E8C8}";
const TICK: &str = "\u{E73E}";
const CROSS: &str = "\u{E711}";
const BIN: &str = "\u{E74D}";

fn icon(glyph: &'static str, size: f32) -> gpui::Div {
    div()
        .font_family("Segoe Fluent Icons")
        .text_size(px(size))
        .line_height(px(size))
        .child(glyph)
}

/// What the capture leaves as: saved, copied, thrown away. Icons only (owner: 「アイコンだけでやれ
/// るようにしたい！」), their names and keys in their tooltips; saving stands out while uploading is
/// not on offer (P5).
struct Actions {
    language: Language,
    /// (Weakly: the editor holds this view.)
    editor: WeakEntity<Editor>,
    clipboard: Option<u32>,
    /// The choice of what the capture leaves as this once, open for one way, and since when (it
    /// comes down into its place).
    choosing: Option<Leaves>,
    opened: Instant,
    /// For which capture it was open: another capture has none open.
    choosing_for: Option<u64>,
    /// The choice was opened by the tour: it is the tour's to put away.
    by_tour: bool,
    /// The keys, taken from the editor while the choice is open.
    keys_held: Option<Subscription>,
    /// Uploading was on offer as the buttons were last drawn (the editor was told so).
    offers_upload: bool,
    marks: perora_ui::tour::Marks,
}

/// The arrow that opens the choice, and how the choice comes down.
const CHEVRON: &str = "\u{E70D}";
const CHOICE_COMES: Duration = Duration::from_millis(160);

impl Actions {
    fn new(language: Language, editor: &Entity<Editor>, cx: &mut Context<Self>) -> Self {
        cx.observe(editor, |_, _, cx| cx.notify()).detach();
        looks_at_clipboard(cx, |this: &mut Self| &mut this.clipboard);
        Self {
            language,
            editor: editor.downgrade(),
            clipboard: platform::clipboard_sequence(),
            choosing: None,
            opened: Instant::now(),
            choosing_for: None,
            by_tour: false,
            keys_held: None,
            offers_upload: false,
            marks: editor.read(cx).marks(),
        }
    }

    /// The small arrow beside a way to leave (the large window's: a list under the row): it opens
    /// or puts away the choice of what the capture leaves as that way, this once.
    fn chevron(&self, leaves: Leaves, id: u64, theme: &Theme, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let open = self.choosing == Some(leaves);
        let what = self.language.text(
            match leaves {
                Leaves::Save => "preview.save",
                Leaves::Upload => "preview.upload",
                Leaves::Copy => "preview.copy",
            },
            &[],
        );
        let said = self.language.text("preview.as", &[("what", &what)]);
        let text = theme.text;
        div()
            .id(SharedString::from(format!("as-{}", leaves.name())))
            .w(px(18.0))
            .h(px(34.0))
            .flex()
            .items_center()
            .justify_center()
            .border_1()
            .border_r_0()
            .border_color(theme.border)
            .rounded_l(px(6.0))
            .bg(theme.surface)
            .text_color(if open { theme.text } else { theme.muted })
            .cursor(CursorStyle::PointingHand)
            .hover(move |style| style.text_color(text))
            .tooltip(tip(said))
            .child(icon(CHEVRON, 9.0))
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_click(cx.listener(move |this, _, _, cx| {
                this.choosing = if open { None } else { Some(leaves) };
                this.opened = Instant::now();
                this.choosing_for = Some(id);
                if !open {
                    cx.defer(move |cx| preview::touch(id, cx));
                }
                cx.notify();
            }))
    }

    /// The choice, under the row from its right end: the kinds of file (ticked for saving, several
    /// at once; the last stays ticked) and how large at most, the one in use with a dot. Chosen,
    /// it stays open; a press elsewhere or Esc puts it away. For this capture, this way only.
    fn menu(
        &self,
        leaves: Leaves,
        id: u64,
        theme: &Theme,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<impl IntoElement + use<>> {
        let (format, kinds, fit) = preview::leaves_now(id, leaves, cx)?;
        let down = if platform::animations_enabled() {
            perora_ui::motion::EASE_OUT.at(perora_ui::motion::along(
                self.opened,
                Duration::ZERO,
                CHOICE_COMES,
                window,
            ))
        } else {
            1.0
        };
        let heading = |text: String| {
            div()
                .mt(px(4.0))
                .mb(px(2.0))
                .mx(px(8.0))
                .text_size(px(11.0))
                .text_color(theme.muted)
                .child(text)
        };
        let accent = theme.accent;
        let hover = theme.border;
        let item = |name: SharedString, label: String, checked: bool, ticked: bool| {
            div()
                .id(name)
                .relative()
                .h(px(28.0))
                .pl(px(26.0))
                .pr(px(8.0))
                .flex()
                .items_center()
                .rounded(px(5.0))
                .cursor(CursorStyle::PointingHand)
                .hover(move |style| style.bg(hover))
                .when(checked, |item| {
                    item.child(if ticked {
                        div()
                            .absolute()
                            .left(px(8.0))
                            .text_color(accent)
                            .child(icon(TICK, 11.0))
                    } else {
                        div()
                            .absolute()
                            .left(px(10.0))
                            .size(px(7.0))
                            .rounded_full()
                            .bg(accent)
                    })
                })
                .child(label)
        };
        let t = |key: &str| self.language.text(key, &[]);
        let mut list = div()
            .id("menu")
            .occlude()
            .min_w(px(200.0))
            .p(px(6.0))
            .flex()
            .flex_col()
            .bg(theme.surface)
            .border_1()
            .border_color(theme.border)
            .rounded(px(8.0))
            .shadow(vec![gpui::BoxShadow {
                color: gpui::Hsla::from(gpui::rgba(0x0000_0047)),
                offset: point(px(0.0), px(8.0)),
                blur_radius: px(24.0),
                spread_radius: px(0.0),
                inset: false,
            }])
            .on_mouse_down_out(cx.listener(|this, _: &MouseDownEvent, _, cx| {
                // (Opened by the tour, it is the tour's to put away.)
                if this.by_tour {
                    return;
                }
                this.choosing = None;
                cx.notify();
            }))
            .child(heading(t("main.output.format")));
        for each in Format::ALL {
            let save = leaves == Leaves::Save;
            let checked = if save { kinds.contains(&each) } else { format == each };
            let now = kinds.clone();
            list = list.child(
                item(
                    SharedString::from(format!("format-{}", each.name())),
                    self.language.text(&format!("main.output.format.{}", each.name()), &[]),
                    checked,
                    save,
                )
                .on_click(move |_, _, cx| {
                    let now = now.clone();
                    cx.defer(move |cx| {
                        preview::choose(
                            id,
                            leaves,
                            |once| match leaves {
                                Leaves::Save => {
                                    let kinds: Vec<Format> = Format::ALL
                                        .into_iter()
                                        .filter(|one| now.contains(one) != (*one == each))
                                        .collect();
                                    if !kinds.is_empty() {
                                        once.formats = Some(kinds);
                                    }
                                }
                                _ => once.format = Some(each),
                            },
                            cx,
                        )
                    });
                }),
            );
        }
        list = list.child(heading(t("main.output.fit")));
        for side in std::iter::once(0).chain(FITS) {
            let label = if side == 0 {
                t("main.output.fit.none")
            } else {
                self.language
                    .text("main.output.fit.within", &[("side", &side.to_string())])
            };
            list = list.child(
                item(SharedString::from(format!("fit-{side}")), label, fit == side, false)
                    .on_click(move |_, _, cx| {
                        cx.defer(move |cx| {
                            preview::choose(
                                id,
                                leaves,
                                |once| once.fit = Some((side != 0).then_some(side)),
                                cx,
                            )
                        })
                    }),
            );
        }
        let list = list.relative().child(self.marks.mark("menu"));
        Some(
            div()
                .absolute()
                .top(px(34.0 + 6.0 - 6.0 * (1.0 - down)))
                .right_0()
                .opacity(down)
                .child(deferred(list).with_priority(2)),
        )
    }
}

/// The window looks at the clipboard now and then: what is on it changes without Perora.
fn looks_at_clipboard<T: 'static>(
    cx: &mut Context<T>,
    seen: impl Fn(&mut T) -> &mut Option<u32> + 'static,
) {
    cx.spawn(async move |this, cx| {
        loop {
            cx.background_executor().timer(LOOKS_AT_CLIPBOARD).await;
            let looked = this.update(cx, |this, cx| {
                let now = platform::clipboard_sequence();
                let seen = seen(this);
                if now != *seen {
                    *seen = now;
                    cx.notify();
                }
            });
            if looked.is_err() {
                return;
            }
        }
    })
    .detach();
}

impl Render for Actions {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::get(cx).clone();
        let t = |key: &str| self.language.text(key, &[]).to_string();
        let Some((id, edits, touring, tour_opens)) = self
            .editor
            .read_with(cx, |editor, _| {
                (
                    editor.capture_id(),
                    editor.edits().to_vec(),
                    editor.touring(),
                    editor.tour_opens_choice(),
                )
            })
            .ok()
        else {
            return div();
        };
        let Some(id) = id else {
            return div();
        };
        // (Another capture has its own choice: none open.)
        if self.choosing_for != Some(id) {
            self.choosing = None;
        }
        // The tour opens the choice beside saving for its step, and puts it away after.
        if tour_opens && self.choosing.is_none() {
            self.choosing = Some(Leaves::Save);
            self.choosing_for = Some(id);
            self.opened = Instant::now();
            self.by_tour = true;
        } else if !tour_opens && self.by_tour {
            self.choosing = None;
            self.by_tour = false;
        }
        let said = preview::said_of(id, &edits, cx);
        let copied = said.as_ref().is_some_and(|said| said.copied);
        let upload_failed = said.as_ref().is_some_and(|said| said.upload_failed);
        let (upload_as, save_as, copy_as) = said
            .map(|said| (said.upload_as, said.save_as, said.copy_as))
            .unwrap_or_default();
        // Uploading, when it is on offer: then it is the one that stands out, and Ctrl+Enter is it
        // (the editor is told, for its tour and its keys).
        let offers_upload = account::uploads(cx);
        if offers_upload != self.offers_upload {
            self.offers_upload = offers_upload;
            let editor = self.editor.clone();
            cx.defer(move |cx| {
                let _ = editor.update(cx, |editor, _| editor.set_offers_upload(offers_upload));
            });
        }
        let button = |name: &'static str| {
            div()
                .id(name)
                .w(px(36.0))
                .h(px(34.0))
                .flex()
                .items_center()
                .justify_center()
                .rounded(theme.radius_small)
                .border_1()
                .border_color(theme.border)
                .bg(theme.surface)
                .cursor(CursorStyle::PointingHand)
        };
        let accent = theme.accent;
        let upload = offers_upload.then(|| {
            button("upload")
                .bg(theme.accent)
                .border_color(theme.accent)
                .text_color(theme.accent_text)
                .tooltip(tip(format!(
                    "{} (Ctrl+Enter) · {upload_as}",
                    t(if upload_failed {
                        "preview.retry"
                    } else {
                        "preview.upload"
                    })
                )))
                .child(icon(UPLOAD, 16.0))
                .on_click(move |_, _, cx| cx.defer(move |cx| preview::upload(id, cx)))
        });
        let save = button("save")
            .when(offers_upload, |save| {
                save.hover(move |style| style.border_color(accent))
            })
            .when(!offers_upload, |save| {
                save.bg(theme.accent)
                    .border_color(theme.accent)
                    .text_color(theme.accent_text)
            })
            .tooltip(tip(format!(
                "{} ({}) · {save_as}",
                t("preview.save"),
                if offers_upload { "Ctrl+S" } else { "Ctrl+S, Ctrl+Enter" }
            )))
            .child(icon(SAVE, 16.0))
            .on_click(move |_, _, cx| cx.defer(move |cx| preview::save(id, cx)));
        // (Named after what it says: GPUI keeps a tooltip that is up as it was made.)
        let copy = button(if copied { "copied" } else { "copy" })
            .hover(move |style| style.border_color(accent))
            .tooltip(tip(if copied {
                t("preview.copied")
            } else {
                format!("{} · {copy_as}", t("preview.copy"))
            }))
            .when(copied, |copy| copy.text_color(theme.good))
            .child(icon(if copied { TICK } else { SHEETS }, 16.0))
            .on_click(move |_, _, cx| cx.defer(move |cx| preview::copy(id, cx)));
        let bin = button("discard")
            .hover(move |style| style.border_color(accent))
            .tooltip(tip(format!("{} (Esc)", t("preview.discard"))))
            .child(icon(BIN, 16.0))
            .on_click(move |_, _, cx| cx.defer(move |cx| preview::discard_by_user(Some(id), cx)));
        let menu = self
            .choosing
            .and_then(|leaves| self.menu(leaves, id, &theme, window, cx));
        // While the choice is open, Esc puts it away and no other key does anything (owner's
        // window: the keys are the editor's otherwise). Not while the tour is on: they are its;
        // not with Alt, which is the system's. Taken before the window hands them out: GPUI hands
        // a key to what has the keyboard (the editor) and what that is inside of, not to what is
        // inside it, as this is. (Asked for while what is drawn is being made, a listener of the
        // window's own has nowhere to be kept, and GPUI ends Perora.)
        if self.choosing.is_none() || touring {
            self.keys_held = None;
        } else if self.keys_held.is_none() {
            let this = cx.entity().downgrade();
            let own = window.window_handle();
            self.keys_held = Some(cx.intercept_keystrokes(move |event, window, cx| {
                if window.window_handle() != own || event.keystroke.modifiers.alt {
                    return;
                }
                cx.stop_propagation();
                if event.keystroke.key == "escape" {
                    let _ = this.update(cx, |this, cx| {
                        this.choosing = None;
                        cx.notify();
                    });
                }
            }));
        }
        let save_as_chevron = div()
            .relative()
            .child(self.chevron(Leaves::Save, id, &theme, cx))
            .child(self.marks.mark("as"));
        let copy_as_chevron = self.chevron(Leaves::Copy, id, &theme, cx);
        let upload = upload.map(|upload| {
            div()
                .flex()
                .child(self.chevron(Leaves::Upload, id, &theme, cx))
                .child(upload.rounded_l(px(0.0)))
        });
        div()
            .relative()
            .flex()
            .gap(px(4.0))
            .child(self.marks.mark("actions"))
            .children(upload)
            .child(
                div()
                    .flex()
                    .child(save_as_chevron)
                    .child(save.rounded_l(px(0.0))),
            )
            .child(
                div()
                    .flex()
                    .child(copy_as_chevron)
                    .child(copy.rounded_l(px(0.0))),
            )
            .child(bin)
            .children(menu)
    }
}

/// What is said under the picture: what went wrong or what is on the clipboard, and how large the
/// capture is with how much is drawn on it of how much a drawing holds.
struct Status {
    language: Language,
    /// (Weakly: the editor holds this view.)
    editor: WeakEntity<Editor>,
    clipboard: Option<u32>,
    marks: perora_ui::tour::Marks,
}

impl Status {
    fn new(language: Language, editor: &Entity<Editor>, cx: &mut Context<Self>) -> Self {
        cx.observe(editor, |_, _, cx| cx.notify()).detach();
        looks_at_clipboard(cx, |this: &mut Self| &mut this.clipboard);
        Self {
            language,
            editor: editor.downgrade(),
            clipboard: platform::clipboard_sequence(),
            marks: editor.read(cx).marks(),
        }
    }
}

impl Render for Status {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::get(cx).clone();
        let t = |key: &str| self.language.text(key, &[]);
        let Some((id, edits, full, kept)) = self
            .editor
            .read_with(cx, |editor, _| {
                (
                    editor.capture_id(),
                    editor.edits().to_vec(),
                    editor.is_full(),
                    editor.shown_size(),
                )
            })
            .ok()
        else {
            return div();
        };
        let Some(id) = id else {
            return div();
        };
        let said = preview::said_of(id, &edits, cx);
        // (Saying "save or upload it as it is" only when uploading is on offer.)
        let full_said = if account::uploads(cx) {
            "editor.full"
        } else {
            "editor.fullWithoutUpload"
        };
        let failure = said
            .as_ref()
            .and_then(|said| said.failure.clone())
            .or_else(|| full.then(|| t(full_said)));
        let first = match failure {
            Some(failure) => div()
                .id("said")
                .min_h(px(18.0))
                .text_color(theme.bad)
                .whitespace_nowrap()
                .overflow_hidden()
                .text_ellipsis()
                .tooltip(tip(failure.clone()))
                .child(failure),
            None => {
                let line = div().id("said").min_h(px(18.0)).flex().items_center();
                match said.as_ref().map(|said| (said.on_clipboard, said.copied)) {
                    Some((true, true)) => line
                        .text_color(theme.good)
                        .child(icon(TICK, 12.0).mr(px(5.0)))
                        .child(t("preview.copied")),
                    Some((true, false)) => line
                        .text_color(theme.muted)
                        .child(icon(CROSS, 12.0).mr(px(5.0)))
                        .child(t("preview.copiedBefore")),
                    _ => line,
                }
            }
        };
        // (The size it leaves at: cut down, what it is cut down to.)
        let (width, height) = kept.unwrap_or_else(|| {
            said.as_ref()
                .map(|said| (said.width, said.height))
                .unwrap_or_default()
        });
        let count = edits.len().to_string();
        let most = perora_core::drawing::MOST_EDITS.to_string();
        div()
            .relative()
            .flex()
            .flex_col()
            .gap(px(2.0))
            .child(self.marks.mark("status"))
            .child(first)
            .child(
            div().text_color(theme.muted).child(format!(
                "{width} × {height} · {}",
                self.language
                    .text("editor.layers", &[("count", &count), ("most", &most)])
            )),
        )
    }
}
