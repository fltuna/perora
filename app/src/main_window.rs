//! The main window: home, settings and account. P1 has only its frame, to show the theme and what
//! was read from the settings; P5 of docs/gpui-rewrite.md fills it in.

use gpui::{
    AnyWindowHandle, App, AppContext, Bounds, Context, IntoElement, ParentElement, Render, Styled,
    TitlebarOptions, Window, WindowBounds, WindowOptions, div, px, size,
};
use perora_ui::{Mode, Theme};

use crate::state::Perora;

/// The size the Tauri version opens at, and the smallest it may be (logical pixels).
const SIZE: (f32, f32) = (1200.0, 800.0);

pub struct MainWindow;

impl Render for MainWindow {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::get(cx);
        let perora = Perora::get(cx);
        let settings = perora.settings();
        let language = perora.language();
        let lines = [
            format!("Perora (GPUI) — {}", language.name()),
            format!(
                "theme: {:?} / shortcut: {} ({})",
                theme.mode,
                settings.shortcut,
                perora
                    .shortcut_label()
                    .unwrap_or_else(|| "not registered".into())
            ),
            format!(
                "settings: {}",
                perora
                    .settings_path
                    .as_ref()
                    .map(|path| path.display().to_string())
                    .unwrap_or_else(|| "-".into())
            ),
        ];
        div()
            .size_full()
            .p_6()
            .flex()
            .flex_col()
            .gap_2()
            .bg(theme.background)
            .text_color(theme.text)
            .font_family(theme.font.clone())
            .text_size(theme.text_size)
            .children(lines.into_iter().map(|line| {
                div()
                    .p_3()
                    .rounded(theme.radius)
                    .bg(theme.surface)
                    .border_1()
                    .border_color(theme.border)
                    .child(line)
            }))
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
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    use windows::Win32::Foundation::HWND;
    use windows::Win32::UI::WindowsAndMessaging::{IsIconic, IsWindowVisible};
    let Ok(handle) = HasWindowHandle::window_handle(window) else {
        return false;
    };
    let RawWindowHandle::Win32(handle) = handle.as_raw() else {
        return false;
    };
    let hwnd = HWND(handle.hwnd.get() as *mut _);
    unsafe { IsWindowVisible(hwnd).as_bool() && !IsIconic(hwnd).as_bool() }
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
    if let Err(error) = cx.open_window(options, |_, cx| cx.new(|_| MainWindow)) {
        eprintln!("the main window could not be opened: {error}");
    }
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
