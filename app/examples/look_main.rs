//! For working on the main window without taking over the screen: the main window in a hidden window
//! (cloaked: never seen, takes no input from the user), driven by a script of presses and keys sent
//! to the window itself, and drawn into PNG files to look at. As `look.rs` does for the editor.
//!
//! `cargo run -p perora-app --profile fast --example look_main -- <script> <out-dir> [en|ja] [light|dark|system] [reveal [WxH]]`
//!
//! With `reveal`, the screen that sets the part of the monitor that brings the mode bar back, over a
//! made-up still, in a window of that size (1200x800 unless said: a monitor that is not 16:9).
//!
//! The settings are a file of their own in `<out-dir>` (made anew each run, with the language and
//! the look asked for), never the user's; nothing reaches the system (`PERORA_DRY_SYSTEM`: no
//! registry, no sound, no other window). No tray, no shortcut, no claim to be the one Perora.
//!
//! The script, a line each (logical pixels of the window, which is 1200 x 800; `#` begins a remark):
//! - `move X Y`, `down X Y`, `up X Y`, `click X Y` (down and up), `twice X Y`, `drag X Y X2 Y2 [STEPS]`,
//!   with `shift`, `ctrl` after them for those held (`right`, `middle` for those buttons)
//! - `wheel X Y LINES` (up is more than 0)
//! - `key K` (a key as GPUI names it: `tab`, `escape`, `ctrl-shift-j`), `type TEXT`
//! - `files X Y` (files dragged in over that point), `over X Y` (moved on), `drop X Y` (let go of
//!   there), `away` (dragged out again)
//! - `wait MS` (the window draws meanwhile, as it does on screen)
//! - `shot NAME` (the window, as it is now, into `<out-dir>/NAME.png`)
//!
//! With `LOOK_TAKEN` set, the shortcut is as if another application had it; `LOOK_SETTINGS` adds to
//! the settings the window starts with (JSON members: `"copyToClipboard": false`); `LOOK_ACCOUNT`
//! makes up the account (`out`, `waiting`, `in`, `in-none`, `in-unreachable`, `in-refused`);
//! `LOOK_UPDATE` a newer version (`found`, `downloading`, `installing`, `failed`, `open`), and `+skipped` after it for a version passed over).

// The application's own modules, as its `main.rs` has them: an example cannot use a program's.
#![allow(dead_code)]

#[path = "../src/account.rs"]
mod account;
#[path = "../src/arguments.rs"]
mod arguments;
#[path = "../src/bar.rs"]
mod bar;
#[path = "../src/burn.rs"]
mod burn;
#[path = "../src/capture.rs"]
mod capture;
#[path = "../src/corner.rs"]
mod corner;
#[path = "../src/cursor.rs"]
mod cursor;
#[path = "../src/display.rs"]
mod display;
#[path = "../src/gpu.rs"]
mod gpu;
#[path = "../src/hwnd.rs"]
mod hwnd;
#[path = "../src/large.rs"]
mod large;
#[path = "../src/main_window.rs"]
mod main_window;
#[path = "../src/notice.rs"]
mod notice;
#[path = "../src/opening.rs"]
mod opening;
#[path = "../src/preview.rs"]
mod preview;
#[path = "../src/selection.rs"]
mod selection;
#[path = "../src/shortcut.rs"]
mod shortcut;
#[path = "../src/state.rs"]
mod state;
#[path = "../src/tray.rs"]
mod tray;
#[path = "../src/update.rs"]
mod update;
#[path = "../src/window_list.rs"]
mod window_list;

use std::path::PathBuf;
use std::time::Duration;

use gpui::{
    AnyWindowHandle, App, AppContext, ExternalPaths, FileDropEvent, Keystroke, Modifiers,
    MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, PlatformInput, Point,
    WindowBackgroundAppearance, WindowBounds, WindowKind, WindowOptions, point, px,
};
use perora_core::geometry::Rect;
use perora_core::platform::{self, Frame, WindowId};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};

use crate::main_window::MainWindow;
use crate::main_window::area_screen::AreaScreen;
use crate::state::Perora;

/// As the application's `main.rs` has them (`tray.rs` sends these).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Signal {
    Capture,
    Open,
    ToggleMain,
    Quit,
    Asked(arguments::Asked),
    ShellBack,
}

/// As the application's `main.rs` has it: never made here (no tray, no shortcut).
struct Kept {
    _tray: tray::Tray,
    _shortcuts: Option<shortcut::Shortcuts>,
}

impl gpui::Global for Kept {}

/// As the application's `main.rs` has it: here, only the window goes.
pub fn quit(cx: &mut App) {
    cx.quit();
}

const SIZE: (u32, u32) = (1200, 800);

fn main() {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    let (Some(script), Some(out)) = (arguments.first(), arguments.get(1)) else {
        eprintln!("look_main <script> <out-dir> [en|ja] [light|dark|system] [reveal [WxH]]");
        std::process::exit(2);
    };
    let script = std::fs::read_to_string(script).expect("the script can be read");
    let out = PathBuf::from(out);
    std::fs::create_dir_all(&out).expect("the out directory can be made");
    let code = arguments.get(2).cloned().unwrap_or_else(|| "ja".into());
    let look = arguments.get(3).cloned().unwrap_or_else(|| "dark".into());
    let reveal = arguments.get(4).is_some_and(|what| what == "reveal");
    let (width, height) = arguments
        .get(5)
        .and_then(|size| size.split_once('x'))
        .and_then(|(width, height)| Some((width.parse().ok()?, height.parse().ok()?)))
        .unwrap_or(SIZE);
    // The settings are a file of their own beside the shots, made anew: not where the user's
    // Perora keeps them.
    let settings = out.join("settings.json");
    // (With a setting of a version to come: what the file holds that this one does not know is to
    // be kept as it is. `LOOK_SETTINGS` adds others, as JSON members: `"copyToClipboard": false`.)
    let more = std::env::var("LOOK_SETTINGS")
        .map(|more| format!(", {more}"))
        .unwrap_or_default();
    std::fs::write(
        &settings,
        format!(
            "{{\"language\": \"{code}\", \"theme\": \"{look}\", \"aSettingToCome\": [1, 2]{more}}}"
        ),
    )
    .expect("the settings can be written");
    // SAFETY: nothing else runs yet to read the environment meanwhile.
    unsafe {
        std::env::set_var("PERORA_SETTINGS", &settings);
        std::env::set_var("PERORA_EDITOR_SETTINGS", out.join("editor-settings.json"));
        std::env::set_var("PERORA_DRY_SYSTEM", "1");
    }

    gpui_platform::application().run(move |cx: &mut App| {
        let mut perora = Perora::load();
        // As the application has them as it starts: what the shell was told (only read), and the
        // shortcut as the system gave it (pretended: none is asked for here; with `LOOK_TAKEN`,
        // another application has it).
        perora.shell_left_alone = platform::hotkeys_left_alone(arguments::SNIPPING_LETTER);
        perora.claimed = shortcut::Claimed {
            own: std::env::var_os("LOOK_TAKEN").is_none(),
            snipping: false,
        };
        cx.set_global(perora);
        // The account as `LOOK_ACCOUNT` says (`out`, `waiting`, `in`, `in-none`, `in-unreachable`,
        // `in-refused`): made up, nothing asked of the server. Without it, no account at all.
        if let Ok(pretended) = std::env::var("LOOK_ACCOUNT") {
            account::pretend(&pretended, &out, cx);
        }
        // A newer version as `LOOK_UPDATE` says (`found`, `downloading`, `installing`, `failed`,
        // `open`): made up, nothing asked of the releases.
        if let Ok(pretended) = std::env::var("LOOK_UPDATE") {
            update::pretend(&pretended, cx);
        }
        main_window::apply_theme(cx);
        perora_ui::text_field::bind_keys(cx);
        let options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(gpui::Bounds::new(
                point(px(0.0), px(0.0)),
                gpui::size(px(width as f32), px(height as f32)),
            ))),
            titlebar: None,
            focus: false,
            show: false,
            kind: WindowKind::PopUp,
            is_movable: false,
            is_resizable: false,
            is_minimizable: false,
            window_background: WindowBackgroundAppearance::Opaque,
            ..Default::default()
        };
        let mut handle = None;
        let mut hidden = |window: &mut gpui::Window| {
            handle = of(window);
            if let Some(handle) = handle {
                platform::set_cloaked(handle, true);
                platform::without_transitions(handle);
            }
        };
        let window: AnyWindowHandle = if reveal {
            let still = selection::drawable(test_frame(width * 2, height * 2)).expect("a still");
            let zone = Perora::get(cx).settings().bar_reveal_area;
            cx.open_window(options, |window, cx| {
                hidden(window);
                cx.new(|cx| AreaScreen::new(still, zone, cx))
            })
            .expect("the window opens")
            .into()
        } else {
            cx.open_window(options, |window, cx| {
                hidden(window);
                cx.new(|cx| MainWindow::new(window, cx))
            })
            .expect("the window opens")
            .into()
        };
        let handle = handle.expect("the window has a handle");
        let any = window;
        let (script, out) = (script.clone(), out.clone());
        cx.spawn(async move |cx| play(script, out, any, handle, (width, height), cx).await)
            .detach();
    });
}

async fn play(
    script: String,
    out: PathBuf,
    any: AnyWindowHandle,
    handle: WindowId,
    (width, height): (u32, u32),
    cx: &mut gpui::AsyncApp,
) {
    let scale = any
        .update(cx, |_, window, _| window.scale_factor())
        .unwrap_or(1.0);
    let (across, down) = sized(
        handle,
        (width as f32 * scale) as u32,
        (height as f32 * scale) as u32,
    );
    eprintln!("look_main: the window is {across}x{down} at {scale}");
    let mut held = None::<MouseButton>;
    let mut at = point(px(0.0), px(0.0));
    let files = || {
        ExternalPaths(
            [PathBuf::from(r"C:\pictures\dropped.png")]
                .into_iter()
                .collect(),
        )
    };
    for (number, line) in script.lines().enumerate() {
        let line = line.split('#').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }
        let words: Vec<&str> = line.split_whitespace().collect();
        let numbers: Vec<f32> = words[1..]
            .iter()
            .filter_map(|word| word.parse().ok())
            .collect();
        let modifiers = Modifiers {
            shift: words.contains(&"shift"),
            control: words.contains(&"ctrl"),
            ..Default::default()
        };
        let button = if words.contains(&"right") {
            MouseButton::Right
        } else if words.contains(&"middle") {
            MouseButton::Middle
        } else {
            MouseButton::Left
        };
        let place = |index: usize| point(px(numbers[index * 2]), px(numbers[index * 2 + 1]));
        let mut events = Vec::new();
        match words[0] {
            "move" => {
                at = place(0);
                events.push(moved(at, held, modifiers));
            }
            "down" => {
                at = place(0);
                events.push(moved(at, held, modifiers));
                events.push(pressed(at, button, modifiers, 1));
                held = Some(button);
            }
            "up" => {
                at = place(0);
                events.push(moved(at, held, modifiers));
                events.push(let_go(at, button, modifiers, 1));
                held = None;
            }
            "click" => {
                at = place(0);
                events.push(moved(at, None, modifiers));
                events.push(pressed(at, button, modifiers, 1));
                events.push(let_go(at, button, modifiers, 1));
            }
            "twice" => {
                at = place(0);
                events.push(moved(at, None, modifiers));
                for count in [1, 2] {
                    events.push(pressed(at, button, modifiers, count));
                    events.push(let_go(at, button, modifiers, count));
                }
            }
            "drag" => {
                let (from, to) = (place(0), place(1));
                let steps = numbers.get(4).copied().unwrap_or(10.0).max(1.0) as usize;
                events.push(moved(from, None, modifiers));
                events.push(pressed(from, button, modifiers, 1));
                for step in 1..=steps {
                    let along = step as f32 / steps as f32;
                    at = point(
                        from.x + (to.x - from.x) * along,
                        from.y + (to.y - from.y) * along,
                    );
                    events.push(moved(at, Some(button), modifiers));
                }
                events.push(let_go(at, button, modifiers, 1));
            }
            "wheel" => {
                at = place(0);
                events.push(moved(at, held, modifiers));
                events.push(PlatformInput::Scroll(gpui::ScrollEvent {
                    position: at,
                    delta: gpui::ScrollDelta::Lines(point(0.0, numbers[2])),
                    modifiers,
                    touch_phase: gpui::TouchPhase::Moved,
                    ..Default::default()
                }));
            }
            "files" => {
                at = place(0);
                events.push(PlatformInput::FileDrop(FileDropEvent::Entered {
                    position: at,
                    paths: files(),
                }));
            }
            "over" => {
                at = place(0);
                events.push(PlatformInput::FileDrop(FileDropEvent::Pending {
                    position: at,
                }));
            }
            "drop" => {
                at = place(0);
                events.push(PlatformInput::FileDrop(FileDropEvent::Submit {
                    position: at,
                }));
                events.push(PlatformInput::FileDrop(FileDropEvent::Ended));
            }
            "away" => events.push(PlatformInput::FileDrop(FileDropEvent::Exited)),
            "key" | "type" | "wait" | "shot" => {}
            other => eprintln!("look_main: line {}: what is {other}?", number + 1),
        }
        for event in events {
            let _ = any.update(cx, |_, window, cx| {
                window.dispatch_event(event, cx);
                window.draw(cx).clear(cx);
            });
        }
        match words[0] {
            "key" => {
                let Some(keystroke) =
                    Keystroke::parse(Box::leak(words[1].to_string().into_boxed_str())).ok()
                else {
                    eprintln!("look_main: line {}: what key is that?", number + 1);
                    continue;
                };
                let _ = any.update(cx, |_, window, cx| {
                    window.dispatch_keystroke(keystroke, cx);
                    window.draw(cx).clear(cx);
                });
            }
            "type" => {
                let text = line["type".len()..].trim().to_string();
                for letter in text.chars() {
                    let keystroke = Keystroke {
                        key: letter.to_string(),
                        key_char: Some(letter.to_string()),
                        modifiers: Modifiers::default(),
                    };
                    let _ = any.update(cx, |_, window, cx| {
                        window.dispatch_keystroke(keystroke, cx);
                        window.draw(cx).clear(cx);
                    });
                }
            }
            "wait" => {
                let until = std::time::Instant::now() + Duration::from_millis(numbers[0] as u64);
                while std::time::Instant::now() < until {
                    let _ = any.update(cx, |_, window, cx| {
                        window.draw(cx).clear(cx);
                    });
                    cx.background_executor()
                        .timer(Duration::from_millis(16))
                        .await;
                }
            }
            "shot" => {
                let drawn = any
                    .update(cx, |_, window, cx| {
                        window.draw(cx).clear(cx);
                        window.render_to_image()
                    })
                    .ok()
                    .and_then(|drawn| drawn.ok());
                match drawn {
                    Some(image) => {
                        let path = out.join(format!("{}.png", words[1]));
                        image.save(&path).expect("the shot is saved");
                        eprintln!("look_main: {}", path.display());
                    }
                    None => eprintln!("look_main: line {}: nothing drawn", number + 1),
                }
            }
            _ => {}
        }
        // (The window's own messages, between lines.)
        cx.background_executor()
            .timer(Duration::from_millis(1))
            .await;
    }
    cx.update(|cx| cx.quit());
}

fn moved(
    at: Point<gpui::Pixels>,
    held: Option<MouseButton>,
    modifiers: Modifiers,
) -> PlatformInput {
    PlatformInput::MouseMove(MouseMoveEvent {
        position: at,
        pressed_button: held,
        modifiers,
    })
}

fn pressed(
    at: Point<gpui::Pixels>,
    button: MouseButton,
    modifiers: Modifiers,
    count: usize,
) -> PlatformInput {
    PlatformInput::MouseDown(MouseDownEvent {
        button,
        position: at,
        modifiers,
        click_count: count,
        first_mouse: false,
    })
}

fn let_go(
    at: Point<gpui::Pixels>,
    button: MouseButton,
    modifiers: Modifiers,
    count: usize,
) -> PlatformInput {
    PlatformInput::MouseUp(MouseUpEvent {
        button,
        position: at,
        modifiers,
        click_count: count,
    })
}

fn of(window: &gpui::Window) -> Option<WindowId> {
    let handle = HasWindowHandle::window_handle(window).ok()?;
    match handle.as_raw() {
        RawWindowHandle::Win32(handle) => Some(WindowId(handle.hwnd.get())),
        _ => None,
    }
}

/// Gives the hidden window its size (physical pixels), behind every other and not active: its inside,
/// as large as the system let it be.
fn sized(window: WindowId, across: u32, down: u32) -> (u32, u32) {
    use windows::Win32::Foundation::{HWND, RECT};
    use windows::Win32::UI::WindowsAndMessaging::{
        AdjustWindowRectEx, GWL_EXSTYLE, GWL_STYLE, GetClientRect, GetWindowLongPtrW, HWND_BOTTOM,
        SWP_NOACTIVATE, SWP_SHOWWINDOW, SetWindowPos, WINDOW_EX_STYLE, WINDOW_STYLE,
    };
    let hwnd = HWND(window.0 as *mut _);
    unsafe {
        let style = WINDOW_STYLE(GetWindowLongPtrW(hwnd, GWL_STYLE) as u32);
        let extended = WINDOW_EX_STYLE(GetWindowLongPtrW(hwnd, GWL_EXSTYLE) as u32);
        let mut outer = RECT {
            left: 0,
            top: 0,
            right: across as i32,
            bottom: down as i32,
        };
        let _ = AdjustWindowRectEx(&mut outer, style, false, extended);
        let _ = SetWindowPos(
            hwnd,
            Some(HWND_BOTTOM),
            0,
            0,
            outer.right - outer.left,
            outer.bottom - outer.top,
            SWP_NOACTIVATE | SWP_SHOWWINDOW,
        );
        let mut inside = RECT::default();
        let _ = GetClientRect(hwnd, &mut inside);
        (
            (inside.right - inside.left).max(1) as u32,
            (inside.bottom - inside.top).max(1) as u32,
        )
    }
}

/// A still to set the part over: soft colours across, darker down, and a grid every 120 pixels.
fn test_frame(width: u32, height: u32) -> Frame {
    let mut rgba = Vec::with_capacity(width as usize * height as usize * 4);
    for y in 0..height {
        for x in 0..width {
            let grid = x % 120 < 2 || y % 120 < 2;
            let light = 1.0 - y as f32 / height as f32 * 0.5;
            let (r, g, b) = if grid {
                (230.0, 230.0, 230.0)
            } else {
                (
                    90.0 + 100.0 * x as f32 / width as f32,
                    120.0,
                    190.0 - 90.0 * x as f32 / width as f32,
                )
            };
            rgba.extend_from_slice(&[(r * light) as u8, (g * light) as u8, (b * light) as u8, 255]);
        }
    }
    Frame {
        area: Rect::new(0, 0, width, height),
        rgba,
    }
}
