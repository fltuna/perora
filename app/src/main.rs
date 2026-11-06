//! Perora on GPUI. The plan is docs/gpui-rewrite.md; what must keep working is docs/gpui-parity.md.
//!
//! P1 so far: the settings and the language, the icon in the notification area, the shortcut, one
//! Perora at a time (later starts hand their arguments over), and the GPU let go of while no window
//! is open.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod arguments;
mod gpu;
mod instance;
mod main_window;
mod shortcut;
mod state;
mod tray;

use std::path::Path;

use gpui::{App, QuitMode};

use crate::arguments::Asked;
use crate::shortcut::Shortcuts;
use crate::state::Perora;
use crate::tray::Tray;

/// What the icon, its menu and the shortcut ask for. They call back on their own, outside GPUI's
/// updates, so they send this; the application acts on it in its own time.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Signal {
    Capture,
    Open,
    ToggleMain,
    Quit,
    /// What a start asked for: this one's, or a later one's handed over.
    Asked(Asked),
}

/// Kept for as long as the application runs: dropping them takes the icon and the shortcut away.
struct Kept {
    _instance: instance::Instance,
    _tray: Tray,
    _shortcuts: Option<Shortcuts>,
}

impl gpui::Global for Kept {}

fn main() {
    // The uninstaller's hook: give back what Perora took, and nothing else (0.1.x's contract).
    if std::env::args_os().any(|argument| argument == arguments::UNINSTALLING) {
        arguments::uninstalling();
        return;
    }
    // A second Perora hands its arguments to the first and ends.
    let Some(instance) = instance::claim() else {
        return;
    };
    let started: Vec<String> = std::env::args_os()
        .map(|argument| argument.to_string_lossy().into_owned())
        .collect();
    // The first start: as in the Tauri version, `--capture` only means something to a Perora that
    // is running; started with it, Perora shows its window.
    let first = match arguments::asked(&started, &std::env::current_dir().unwrap_or_default()) {
        Asked::Capture => Asked::Main,
        other => other,
    };

    gpui_platform::application()
        // An application of the notification area: no window is open while it waits.
        .with_quit_mode(QuitMode::Explicit)
        .with_windowing(gpu::AT_START)
        .run(|cx: &mut App| {
            let mut perora = Perora::load();
            let (sender, receiver) = async_channel::unbounded::<Signal>();

            // The shortcut the settings name, or the default when theirs means nothing (and the
            // settings say so from here on, as in the Tauri version).
            let combination = shortcut::chosen(&perora.settings().shortcut);
            perora
                .settings
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .shortcut = combination.clone();
            let take_snipping = perora.settings().take_snipping;
            // The settings want the system's combination for snipping, and the shell was told to
            // take it back meanwhile (by an uninstall that was part of installing again, say): it
            // is told to leave it alone again, from the next time it starts.
            if take_snipping
                && !perora_core::platform::hotkeys_left_alone(arguments::SNIPPING_LETTER)
                && let Err(detail) =
                    perora_core::platform::leave_hotkeys_alone(arguments::SNIPPING_LETTER, true)
            {
                eprintln!("the shell could not be told about its combinations: {detail}");
            }
            perora.shell_left_alone =
                perora_core::platform::hotkeys_left_alone(arguments::SNIPPING_LETTER);
            let mut shortcuts = Shortcuts::new();
            if let Some(shortcuts) = shortcuts.as_mut() {
                perora.claimed = shortcuts.claim(&combination, take_snipping);
            }
            let pressed = sender.clone();
            Shortcuts::on_pressed(move || {
                let _ = pressed.try_send(Signal::Capture);
            });

            let language = perora.language();
            let tray = Tray::new(
                &language,
                perora.shortcut_label().as_deref(),
                &perora.shortcut_shown(),
            );
            // Without its icon, Perora could be neither found nor ended: it does not start (as the
            // Tauri version, whose start failed with it).
            let Some(tray) = tray else {
                eprintln!("the icon could not be put in the notification area");
                cx.quit();
                return;
            };
            tray.listen(sender.clone());
            let later = sender.clone();
            instance::listen(move |forwarded| {
                let asked = arguments::asked(&forwarded.arguments, Path::new(&forwarded.folder));
                let _ = later.try_send(Signal::Asked(asked));
            });
            let _ = sender.try_send(Signal::Asked(first));
            cx.set_global(perora);
            cx.set_global(Kept {
                _instance: instance,
                _tray: tray,
                _shortcuts: shortcuts,
            });
            main_window::apply_theme(cx);

            cx.spawn(async move |cx| {
                while let Ok(signal) = receiver.recv().await {
                    cx.update(|cx| act(signal, cx));
                }
            })
            .detach();

            // Waiting in the tray needs no GPU (started headless); let go of it again a while
            // after the last window closes.
            gpu::release_after_windows(cx);
        });
}

fn act(signal: Signal, cx: &mut App) {
    match signal {
        // P2: the selection screen. Until then the main window opens, so that the shortcut can be
        // seen to work.
        Signal::Capture => main_window::open(cx),
        Signal::Open => main_window::open(cx),
        Signal::ToggleMain => main_window::toggle(cx),
        Signal::Asked(Asked::Main) => main_window::open(cx),
        Signal::Asked(Asked::Tray) => {}
        Signal::Asked(Asked::Capture) => act(Signal::Capture, cx),
        // P5: opening an image in the editor.
        Signal::Asked(Asked::Open(file)) => eprintln!("open {}: not yet (P5)", file.display()),
        Signal::Quit => {
            // The icon, the shortcut and the claim to be the one Perora go before the application
            // does.
            if cx.has_global::<Kept>() {
                cx.remove_global::<Kept>();
            }
            cx.quit();
        }
    }
}
