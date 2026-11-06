//! For working on the editor without taking over the screen: the editor in a hidden window (cloaked:
//! never seen, takes no input from the user), driven by a script of presses and keys sent to the
//! window itself, and drawn into PNG files to look at.
//!
//! `cargo run -p perora-app --profile fast --example look -- <script> <out-dir> [en|ja]`
//!
//! The script, a line each (logical pixels of the window, which is 1200 x 800; `#` begins a remark):
//! - `move X Y`, `down X Y`, `up X Y`, `click X Y` (down and up), `drag X Y X2 Y2 [STEPS]`, with
//!   `shift`, `ctrl` after them for those held (`right`, `middle` for those buttons)
//! - `key K` (a key as GPUI names it: `r`, `escape`, `ctrl-z`), `type TEXT`
//! - `wait MS` (the window draws meanwhile, as it does on screen)
//! - `shot NAME` (the window, as it is now, into `<out-dir>/NAME.png`)

use std::path::PathBuf;
use std::rc::Rc;
use std::time::Duration;

use gpui::{
    AnyWindowHandle, App, AppContext, Keystroke, Modifiers, MouseButton, MouseDownEvent,
    MouseMoveEvent, MouseUpEvent, PlatformInput, Point, WindowBackgroundAppearance, WindowBounds,
    WindowKind, WindowOptions, point, px,
};
use perora_core::geometry::Rect;
use perora_core::language::Languages;
use perora_core::platform::{self, Frame, WindowId};
use perora_editor::{Capture, Editor, Picture};
use perora_ui::{Mode, Theme};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};

const WIDTH: u32 = 1200;
const HEIGHT: u32 = 800;

fn main() {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    let (Some(script), Some(out)) = (arguments.first(), arguments.get(1)) else {
        eprintln!("look <script> <out-dir> [en|ja]");
        std::process::exit(2);
    };
    let script = std::fs::read_to_string(script).expect("the script can be read");
    let out = PathBuf::from(out);
    std::fs::create_dir_all(&out).expect("the out directory can be made");
    let code = arguments.get(2).cloned().unwrap_or_else(|| "ja".into());

    gpui_platform::application().run(move |cx: &mut App| {
        cx.set_global(Theme::new(Mode::Dark, platform::accent_colour()));
        perora_ui::text_field::bind_keys(cx);
        let language = Languages::load(None)
            .by_code(&code)
            .expect("the language is built in");
        let options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(gpui::Bounds::new(
                point(px(0.0), px(0.0)),
                gpui::size(px(WIDTH as f32), px(HEIGHT as f32)),
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
        let window = cx
            .open_window(options, |window, cx| {
                handle = of(window);
                if let Some(handle) = handle {
                    platform::set_cloaked(handle, true);
                    platform::without_transitions(handle);
                }
                cx.new(|cx| Editor::new(language, window, cx))
            })
            .expect("the window opens");
        let handle = handle.expect("the window has a handle");
        let any: AnyWindowHandle = window.into();
        let (script, out) = (script.clone(), out.clone());
        cx.spawn(async move |cx| play(script, out, window, any, handle, cx).await)
            .detach();
    });
}

async fn play(
    script: String,
    out: PathBuf,
    window: gpui::WindowHandle<Editor>,
    any: AnyWindowHandle,
    handle: WindowId,
    cx: &mut gpui::AsyncApp,
) {
    {
        let scale = any
            .update(cx, |_, window, _| window.scale_factor())
            .unwrap_or(1.0);
        let (across, down) = sized(
            handle,
            (WIDTH as f32 * scale) as u32,
            (HEIGHT as f32 * scale) as u32,
        );
        eprintln!("look: the window is {across}x{down} at {scale}");
        let picture = Picture::new(&test_frame(1920, 1080)).expect("a picture");
        let _ = window.update(cx, |editor, window, cx| {
            editor.show(
                Capture {
                    id: 1,
                    picture: Rc::new(picture),
                },
                cx,
            );
            window.focus(&gpui::Focusable::focus_handle(editor, cx), cx);
        });
        let mut held = None::<MouseButton>;
        let mut at = point(px(0.0), px(0.0));
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
                    events.push(pressed(at, button, modifiers));
                    held = Some(button);
                }
                "up" => {
                    at = place(0);
                    events.push(moved(at, held, modifiers));
                    events.push(let_go(at, button, modifiers));
                    held = None;
                }
                "click" => {
                    at = place(0);
                    events.push(moved(at, None, modifiers));
                    events.push(pressed(at, button, modifiers));
                    events.push(let_go(at, button, modifiers));
                }
                "drag" => {
                    let (from, to) = (place(0), place(1));
                    let steps = numbers.get(4).copied().unwrap_or(10.0).max(1.0) as usize;
                    events.push(moved(from, None, modifiers));
                    events.push(pressed(from, button, modifiers));
                    for step in 1..=steps {
                        let along = step as f32 / steps as f32;
                        at = point(
                            from.x + (to.x - from.x) * along,
                            from.y + (to.y - from.y) * along,
                        );
                        events.push(moved(at, Some(button), modifiers));
                    }
                    events.push(let_go(at, button, modifiers));
                }
                "key" | "type" | "wait" | "shot" => {}
                other => eprintln!("look: line {}: what is {other}?", number + 1),
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
                        eprintln!("look: line {}: what key is that?", number + 1);
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
                    let until =
                        std::time::Instant::now() + Duration::from_millis(numbers[0] as u64);
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
                            eprintln!("look: {}", path.display());
                        }
                        None => eprintln!("look: line {}: nothing drawn", number + 1),
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

fn pressed(at: Point<gpui::Pixels>, button: MouseButton, modifiers: Modifiers) -> PlatformInput {
    PlatformInput::MouseDown(MouseDownEvent {
        button,
        position: at,
        modifiers,
        click_count: 1,
        first_mouse: false,
    })
}

fn let_go(at: Point<gpui::Pixels>, button: MouseButton, modifiers: Modifiers) -> PlatformInput {
    PlatformInput::MouseUp(MouseUpEvent {
        button,
        position: at,
        modifiers,
        click_count: 1,
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

/// A picture to draw on: soft colours across, darker down, and a grid every 120 pixels.
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
