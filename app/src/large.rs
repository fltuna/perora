//! The large window: the capture to draw on (the editor), with what it leaves as at the top right
//! and what is said of it under the picture. A click on the small preview's picture brings the
//! capture here, or it comes here at once (`afterCapture: edit`). Closing it throws the capture away,
//! as the bin does.
//!
//! The window is opened as the capture comes and closed as it goes (the GPU is held while it is
//! open), and only hidden while a capture is taken, so that what was drawn stays.
//!
//! P4 of docs/gpui-rewrite.md, first part. Not yet: the choice of what it leaves as this once (⌄),
//! uploading (P5), the fade it opened with in the Tauri version, asking before a new capture takes
//! the place of a drawing, the tour's i (P6).

use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use gpui::prelude::FluentBuilder;
use gpui::{
    App, AppContext, Bounds, Context, CursorStyle, Entity, Focusable, Global, InteractiveElement,
    IntoElement, ParentElement, Render, SharedString, StatefulInteractiveElement, Styled,
    Subscription, TitlebarOptions, WeakEntity, Window, WindowBounds, WindowHandle, WindowOptions,
    div, px, size,
};
use perora_core::edit::Edit;
use perora_core::language::Language;
use perora_core::platform::{self, Frame, WindowId};
use perora_editor::{Capture, Editor, Event, Leave, Picture};
use perora_ui::{Theme, Tip};

use crate::state::Perora;
use crate::{gpu, hwnd, preview};

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
    if let Some(window) = cx.default_global::<Large>().window {
        let _ = window.update(cx, move |editor, window, cx| {
            editor.show(capture, cx);
            if let Some(handle) = hwnd::of(window) {
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
    let opened = cx.open_window(options, |window, cx| {
        // Closed (its button, Alt+F4): thrown away, as the bin does.
        window.on_window_should_close(cx, |_, cx| {
            cx.defer(|cx| preview::discard_by_user(None, cx));
            false
        });
        let editor = cx.new(|cx| {
            let mut editor = Editor::new(language.clone(), window, cx);
            editor.show(capture, cx);
            editor
        });
        let actions = cx.new(|cx| Actions::new(language.clone(), &editor, cx));
        let status = cx.new(|cx| Status::new(language.clone(), &editor, cx));
        editor.update(cx, |editor, _| {
            editor.top_right = Some(actions.into());
            editor.below = Some(status.into());
        });
        window.focus(&editor.focus_handle(cx), cx);
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
                .map(|editor| cx.subscribe(&editor, |editor, event, cx| asked(editor, *event, cx)));
            let large = cx.default_global::<Large>();
            large.window = Some(window);
            large._subscription = subscription;
        }
        Err(error) => eprintln!("the large window could not be opened: {error}"),
    }
}

/// What the editor asks of the window.
fn asked(editor: Entity<Editor>, event: Event, cx: &mut App) {
    let Some(id) = editor.read(cx).capture_id() else {
        return;
    };
    match event {
        Event::Changed => changed(cx),
        Event::Leave(Leave::Save) => preview::save(id, cx),
        Event::Discard => preview::discard_by_user(Some(id), cx),
    }
}

/// Closes the window (its capture is gone).
pub fn close(cx: &mut App) {
    let large = cx.default_global::<Large>();
    large.showing += 1;
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
            hwnd::of(window)
        })
        .ok()
        .flatten();
    let Some(handle) = handle.filter(|&handle| hwnd::on_screen(handle)) else {
        return false;
    };
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
}

impl Actions {
    fn new(language: Language, editor: &Entity<Editor>, cx: &mut Context<Self>) -> Self {
        cx.observe(editor, |_, _, cx| cx.notify()).detach();
        looks_at_clipboard(cx, |this: &mut Self| &mut this.clipboard);
        Self {
            language,
            editor: editor.downgrade(),
            clipboard: platform::clipboard_sequence(),
        }
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
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::get(cx).clone();
        let t = |key: &str| self.language.text(key, &[]);
        let Some((id, edits)) = self
            .editor
            .read_with(cx, |editor, _| {
                (editor.capture_id(), editor.edits().to_vec())
            })
            .ok()
        else {
            return div();
        };
        let Some(id) = id else {
            return div();
        };
        let said = preview::said_of(id, &edits, cx);
        let copied = said.as_ref().is_some_and(|said| said.copied);
        let (save_as, copy_as) = said
            .map(|said| (said.save_as, said.copy_as))
            .unwrap_or_default();
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
        let save = button("save")
            .bg(theme.accent)
            .border_color(theme.accent)
            .text_color(theme.accent_text)
            .tooltip(tip(format!("{} (Ctrl+S) · {save_as}", t("preview.save"))))
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
        div().flex().gap(px(4.0)).child(save).child(copy).child(bin)
    }
}

/// What is said under the picture: what went wrong or what is on the clipboard, and how large the
/// capture is with how much is drawn on it of how much a drawing holds.
struct Status {
    language: Language,
    /// (Weakly: the editor holds this view.)
    editor: WeakEntity<Editor>,
    clipboard: Option<u32>,
}

impl Status {
    fn new(language: Language, editor: &Entity<Editor>, cx: &mut Context<Self>) -> Self {
        cx.observe(editor, |_, _, cx| cx.notify()).detach();
        looks_at_clipboard(cx, |this: &mut Self| &mut this.clipboard);
        Self {
            language,
            editor: editor.downgrade(),
            clipboard: platform::clipboard_sequence(),
        }
    }
}

impl Render for Status {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::get(cx).clone();
        let t = |key: &str| self.language.text(key, &[]);
        let Some((id, edits, full)) = self
            .editor
            .read_with(cx, |editor, _| {
                (
                    editor.capture_id(),
                    editor.edits().to_vec(),
                    editor.is_full(),
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
        // (P5: "editor.full" when uploading is on offer.)
        let failure = said
            .as_ref()
            .and_then(|said| said.failure.clone())
            .or_else(|| full.then(|| t("editor.fullWithoutUpload")));
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
        let (width, height) = said
            .as_ref()
            .map(|said| (said.width, said.height))
            .unwrap_or_default();
        let count = edits.len().to_string();
        let most = perora_core::drawing::MOST_EDITS.to_string();
        div().flex().flex_col().gap(px(2.0)).child(first).child(
            div().text_color(theme.muted).child(format!(
                "{width} × {height} · {}",
                self.language
                    .text("editor.layers", &[("count", &count), ("most", &most)])
            )),
        )
    }
}
