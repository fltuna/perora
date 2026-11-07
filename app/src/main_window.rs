//! The main window: the pages on the left, the chosen page on the right (home, the settings in five
//! pages, the account), as the Tauri version's `main_window.tsx`. Everything set here takes effect at
//! once: there is nothing to save. It opens on the home page, whatever page it was closed on.
//!
//! Closing it leaves Perora in the notification area; a click on the icon there brings it back, and
//! puts it away again. An image dropped on it opens in the editor.

// (Each part has its path said: `examples/look_main.rs` takes this file in by its path, and a file
// taken in so looks for its parts beside itself.)
#[path = "main_window/account_page.rs"]
mod account_page;
#[path = "main_window/area_screen.rs"]
pub mod area_screen;
#[path = "main_window/pages.rs"]
mod pages;
#[path = "main_window/reveal_area.rs"]
mod reveal_area;
#[path = "main_window/rows.rs"]
mod rows;
#[path = "main_window/shortcut_page.rs"]
mod shortcut_page;
#[path = "main_window/system.rs"]
mod system;

use std::path::PathBuf;

use gpui::prelude::FluentBuilder;
use gpui::{
    AnyWindowHandle, App, AppContext, Bounds, Context, DispatchPhase, DragMoveEvent, Entity,
    ExternalPaths, FocusHandle, FontWeight, InteractiveElement, IntoElement, KeyDownEvent,
    MouseUpEvent, ParentElement, Render, ScrollHandle, SharedString, StatefulInteractiveElement,
    Styled, Subscription, TitlebarOptions, Window, WindowBounds, WindowOptions, canvas, div, point,
    px, relative, size,
};
use perora_core::platform::WindowId;
use perora_core::settings::{Settings, seconds_to_set};
use perora_ui::{Mode, Seconds, SecondsEvent, Theme, glyph};

use crate::hwnd;
use crate::state::Perora;

pub use system::Refusal;

/// The size the Tauri version opens at, and the smallest it may be (logical pixels).
const SIZE: (f32, f32) = (1200.0, 800.0);

/// The pages, in the order of the list on the left.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Page {
    Home,
    Capture,
    Save,
    Editor,
    Shortcut,
    General,
    Account,
}

impl Page {
    pub const ALL: [Page; 7] = [
        Page::Home,
        Page::Capture,
        Page::Save,
        Page::Editor,
        Page::Shortcut,
        Page::General,
        Page::Account,
    ];

    /// As the language files name it (`main.nav.<name>`).
    pub const fn name(self) -> &'static str {
        match self {
            Page::Home => "home",
            Page::Capture => "capture",
            Page::Save => "save",
            Page::Editor => "editor",
            Page::Shortcut => "shortcut",
            Page::General => "general",
            Page::Account => "account",
        }
    }

    /// Its glyph in Segoe Fluent Icons, as the Tauri version has it.
    const fn icon(self) -> &'static str {
        match self {
            Page::Home => "\u{E80F}",
            Page::Capture => "\u{E722}",
            Page::Save => "\u{E74E}",
            Page::Editor => "\u{E70F}",
            Page::Shortcut => "\u{E765}",
            Page::General => "\u{E713}",
            Page::Account => "\u{E77B}",
        }
    }
}

/// The three numbers of seconds that are typed: how long the mode bar stays, the small preview, and
/// a notice.
pub(crate) struct SecondsFields {
    pub bar_hide: Entity<Seconds>,
    pub preview: Entity<Seconds>,
    pub notice: Entity<Seconds>,
}

pub struct MainWindow {
    page: Page,
    focus: FocusHandle,
    scroll: ScrollHandle,
    /// A new shortcut is being recorded: the next combination pressed is it.
    recording: bool,
    /// The first of the files being dragged over the window, while they are.
    dragged: Option<PathBuf>,
    /// The last combination that was not made the shortcut, and why.
    refusal: Option<(Refusal, String)>,
    seconds: SecondsFields,
    /// The upload whose link was just copied on the account page: its button says so a moment.
    copied_link: Option<String>,
    _subscriptions: Vec<Subscription>,
}

impl MainWindow {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let settings = Perora::get(cx).settings();
        let unit: SharedString = Perora::get(cx)
            .language()
            .text("main.general.seconds", &[])
            .into();
        let mut subscriptions = Vec::new();
        let mut field = |seconds: f64, set: fn(&mut Settings, f64), cx: &mut Context<Self>| {
            let field = cx.new(|cx| Seconds::new(seconds, unit.clone(), cx));
            subscriptions.push(cx.subscribe(&field, move |_, _, event: &SecondsEvent, cx| {
                let SecondsEvent::Set(seconds) = *event;
                if let Some(seconds) = seconds_to_set(seconds) {
                    system::set(cx, |settings| set(settings, seconds));
                }
            }));
            field
        };
        let seconds = SecondsFields {
            bar_hide: field(
                settings.bar_hide_seconds,
                |settings, seconds| settings.bar_hide_seconds = seconds,
                cx,
            ),
            preview: field(
                settings.preview_seconds,
                |settings, seconds| settings.preview_seconds = seconds,
                cx,
            ),
            notice: field(
                settings.notice_seconds,
                |settings, seconds| settings.notice_seconds = seconds,
                cx,
            ),
        };
        // Away from the window, a shortcut is not being recorded any more (as in the Tauri version:
        // what was pressed elsewhere was not meant for it). Back to it, the system's accent colour
        // is read again: it may have changed meanwhile.
        subscriptions.push(cx.observe_window_activation(window, |this, window, cx| {
            if window.is_window_active() {
                apply_theme(cx);
                // (Uploads may have been made meanwhile, here or elsewhere.)
                if this.page == Page::Account {
                    crate::account::ask_recent(cx);
                }
                cx.notify();
            } else if this.recording {
                this.stop_recording(cx);
            }
        }));
        // The system went light or dark: so do the windows, when the settings follow the system.
        subscriptions.push(cx.observe_window_appearance(window, |_, _, cx| {
            apply_theme(cx);
            system::changed(cx);
        }));
        // Closed while a shortcut was being recorded: the shortcut is Perora's again.
        subscriptions.push(cx.on_release(|this, cx| {
            if this.recording {
                system::suspend_shortcut(false, cx);
            }
        }));
        Self {
            page: Page::Home,
            focus: cx.focus_handle(),
            scroll: ScrollHandle::new(),
            recording: false,
            dragged: None,
            refusal: None,
            seconds,
            copied_link: None,
            _subscriptions: subscriptions,
        }
    }

    fn show_page(&mut self, page: Page, cx: &mut Context<Self>) {
        if self.page == page {
            return;
        }
        if self.recording {
            self.stop_recording(cx);
        }
        self.page = page;
        if page == Page::Account {
            crate::account::ask_recent(cx);
        }
        // (What was said of the last combination tried belongs to the shortcut page as it was.)
        self.refusal = None;
        self.scroll.set_offset(point(px(0.0), px(0.0)));
        cx.notify();
    }

    /// Files dragged over the window: which, for when they are let go of.
    fn dragging(
        &mut self,
        event: &DragMoveEvent<ExternalPaths>,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.dragged = event.drag(cx).paths().first().cloned();
    }

    /// Files let go of over the window: the first opens in the editor.
    fn dropped(&mut self, cx: &mut Context<Self>) {
        if let Some(path) = self.dragged.take() {
            system::open_file(path, cx);
        }
    }

    /// Tab and Shift + Tab go from one control to the next, as in a browser.
    fn key_down(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let keystroke = &event.keystroke;
        if keystroke.key != "tab" || keystroke.modifiers.control || keystroke.modifiers.alt {
            return;
        }
        if keystroke.modifiers.shift {
            window.focus_prev(cx);
        } else {
            window.focus_next(cx);
        }
    }

    fn sidebar(&self, theme: &Theme, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let perora = Perora::get(cx);
        let language = perora.language();
        let version = language.text(
            "main.about.version",
            &[("version", env!("CARGO_PKG_VERSION"))],
        );
        let current = self.page;
        let items: Vec<_> = Page::ALL
            .into_iter()
            .map(|page| {
                let label = language.text(&format!("main.nav.{}", page.name()), &[]);
                let chosen = page == current;
                div()
                    .id(page.name())
                    .relative()
                    .flex()
                    .items_center()
                    .gap(px(14.0))
                    .w_full()
                    .h(px(36.0))
                    .px(px(12.0))
                    .rounded(theme.radius_small)
                    .cursor_pointer()
                    .hover(|style| style.bg(theme.surface_hover))
                    .when(chosen, |item| {
                        // The mark of the page shown: a short bar in the accent colour at its left.
                        item.bg(theme.surface_hover).child(
                            div()
                                .absolute()
                                .left(px(0.0))
                                .top(px(10.0))
                                .bottom(px(10.0))
                                .w(px(3.0))
                                .rounded(px(2.0))
                                .bg(theme.accent),
                        )
                    })
                    .on_click(cx.listener(move |this, _, _, cx| this.show_page(page, cx)))
                    .child(glyph(page.icon(), 16.0))
                    .child(label)
            })
            .collect();
        div()
            .flex_none()
            .w(px(220.0))
            .flex()
            .flex_col()
            .px(px(6.0))
            .py(px(12.0))
            .child(
                div()
                    .flex_1()
                    .flex()
                    .flex_col()
                    .gap(px(2.0))
                    .children(items),
            )
            .child(
                div()
                    .px(px(12.0))
                    .text_size(px(12.0))
                    .text_color(theme.muted)
                    .child(version),
            )
    }
}

impl Render for MainWindow {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::get(cx).clone();
        let perora = Perora::get(cx);
        let settings = perora.settings();
        let language = perora.language();
        let unit: SharedString = language.text("main.general.seconds", &[]).into();
        for (field, seconds) in [
            (&self.seconds.bar_hide, settings.bar_hide_seconds),
            (&self.seconds.preview, settings.preview_seconds),
            (&self.seconds.notice, settings.notice_seconds),
        ] {
            let unit = unit.clone();
            field.update(cx, |field, cx| field.show(seconds, unit, window, cx));
        }

        let title = language.text(&format!("main.nav.{}", self.page.name()), &[]);
        let content = match self.page {
            Page::Home => pages::home(&settings, &language, &theme, cx),
            Page::Capture => pages::capture(self, &settings, &language, &theme, window, cx),
            Page::Save => pages::save(&settings, &language, &theme, cx),
            Page::Editor => pages::editor(&settings, &language, &theme),
            Page::Shortcut => shortcut_page::page(self, &language, &theme, cx),
            Page::General => pages::general(self, &settings, &language, &theme, cx),
            Page::Account => account_page::page(self, &language, &theme, cx),
        };
        let heading = div()
            .flex_none()
            .mb(px(12.0))
            .pb(px(12.0))
            .border_b_2()
            .border_color(theme.border)
            .font_family("Segoe UI Variable Display")
            .text_size(px(28.0))
            .font_weight(FontWeight::SEMIBOLD)
            .line_height(relative(1.3))
            .child(title);
        let page = div()
            .id("page")
            .flex_1()
            .min_w_0()
            .overflow_y_scroll()
            .track_scroll(&self.scroll)
            .pt(px(20.0))
            .px(px(28.0))
            .pb(px(28.0))
            .flex()
            .flex_col()
            .gap(px(4.0))
            .bg(rows::mix(theme.surface, theme.background, 0.55))
            .rounded_tl(theme.radius)
            .border_l_1()
            .border_t_1()
            .border_color(theme.border)
            .child(heading)
            .children(content);

        // An image dragged over the window: dropped, it opens in the editor.
        // (Files dragged over another of Perora's windows are that window's.)
        if !cx.has_active_drag() {
            self.dragged = None;
        }
        let dropping = self.dragged.is_some();
        // (Let go of anywhere over the window, whatever is under it and whatever was pressed last:
        // GPUI's own dropping asks whether the pointer is over what it is dropped on, which it
        // takes not to be after a key was pressed.)
        let main = cx.entity();
        let letting_go = canvas(
            |_, _, _| {},
            move |_, _, window, _| {
                window.on_mouse_event(move |_: &MouseUpEvent, phase, _, cx| {
                    if phase == DispatchPhase::Bubble && cx.has_active_drag() {
                        main.update(cx, |main, cx| main.dropped(cx));
                    }
                });
            },
        )
        .absolute()
        .size_0();
        let drop_said = language.text("preview.drop", &[]);
        div()
            .id("main")
            .size_full()
            .relative()
            .flex()
            .flex_col()
            .bg(theme.background)
            .text_color(theme.text)
            .font_family(theme.font.clone())
            .text_size(theme.text_size)
            .line_height(relative(1.43))
            .track_focus(&self.focus)
            .capture_key_down(cx.listener(Self::recorded))
            .on_key_down(cx.listener(Self::key_down))
            .on_drag_move(cx.listener(Self::dragging))
            .child(letting_go)
            // (Above the pages, the band that offers a newer version goes here: P7, the updater.)
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .child(self.sidebar(&theme, cx))
                    .child(page),
            )
            .when(dropping, |main| {
                main.child(
                    div()
                        .absolute()
                        .top(px(8.0))
                        .left(px(8.0))
                        .right(px(8.0))
                        .bottom(px(8.0))
                        .flex()
                        .items_center()
                        .justify_center()
                        .text_size(px(18.0))
                        .bg(gpui::Hsla {
                            a: 0.82,
                            ..theme.background
                        })
                        .border_2()
                        .border_dashed()
                        .border_color(theme.accent)
                        .rounded(theme.radius)
                        .child(
                            div()
                                .px(px(20.0))
                                .py(px(12.0))
                                .bg(theme.surface)
                                .rounded(theme.radius)
                                .child(drop_said),
                        ),
                )
            })
    }
}

/// Puts the main window away when it is on screen; otherwise shows it (also when it is minimized,
/// as in the Tauri version: one click brings it back).
pub fn toggle(cx: &mut App) {
    if let Some(window) = find(cx) {
        let on_screen = window
            .update(cx, |_, window, _| on_screen(window))
            .unwrap_or(false);
        if on_screen {
            let _ = window.update(cx, |_, window, _| window.remove_window());
            return;
        }
    }
    open(cx);
}

/// Shown, and not minimized.
fn on_screen(window: &Window) -> bool {
    hwnd::of(window).is_some_and(hwnd::on_screen)
}

/// The main window, when it is on screen: what a capture takes Perora's own windows out of the
/// picture by, or puts in it.
pub fn on_screen_now(cx: &mut App) -> Option<WindowId> {
    let window = find(cx)?;
    window
        .update(cx, |_, window, _| {
            hwnd::of(window).filter(|&id| hwnd::on_screen(id))
        })
        .ok()
        .flatten()
}

/// Out of the way of a capture, at once: without the system's fade, which would be in the picture
/// (as in the Tauri version, which also put the window aside this way). The window, when it was on
/// screen, for `come_back`.
pub fn step_aside(cx: &mut App) -> Option<WindowId> {
    let window = on_screen_now(cx)?;
    perora_core::platform::set_transitions(window, false);
    hwnd::hide(window);
    Some(window)
}

/// Back after the capture: as it was put aside, and with the system's animations again.
pub fn come_back(window: WindowId) {
    hwnd::show(window);
    perora_core::platform::set_transitions(window, true);
}

pub fn open(cx: &mut App) {
    if let Some(window) = find(cx) {
        let _ = window.update(cx, |_, window, _| window.activate_window());
        return;
    }
    crate::gpu::attach(cx);
    apply_theme(cx);
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
    if let Err(error) = cx.open_window(options, |window, cx| {
        cx.new(|cx| MainWindow::new(window, cx))
    }) {
        eprintln!("the main window could not be opened: {error}");
    }
}

/// Whether the main window is open on the account page.
pub fn on_account_page(cx: &mut App) -> bool {
    find(cx)
        .and_then(|window| window.downcast::<MainWindow>())
        .and_then(|window| window.read_with(cx, |main, _| main.page == Page::Account).ok())
        .unwrap_or(false)
}

fn find(cx: &App) -> Option<AnyWindowHandle> {
    cx.windows()
        .into_iter()
        .find(|window| window.downcast::<MainWindow>().is_some())
}

/// The theme for the look the settings ask for, in the system's light or dark and accent colour.
pub fn apply_theme(cx: &mut App) {
    let look = Perora::get(cx).settings().look;
    let mode = Mode::of(look, cx.window_appearance());
    cx.set_global(Theme::new(mode, perora_core::platform::accent_colour()));
}
