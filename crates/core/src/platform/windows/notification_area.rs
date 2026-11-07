//! Windows: Perora's icon in the notification area, when the shell starts anew (Explorer restarted,
//! from Perora's settings, the Task Manager or a crash). The shell takes every icon with it, and
//! says so to every window as it is back ("TaskbarCreated"); the window of the icon (tray-icon's)
//! puts it back when it hears that, but Perora's did not come back on the owner's machine either
//! way Explorer was restarted (2026-11-07), although a program of a few lines beside it, with a
//! window like it, heard the shell and put its icon back. Said to the window by Perora itself, the
//! icon comes back. So Perora watches the shell itself: when the one there ends and another is
//! there, it asks whether its icon is, and puts it back until it is.

use std::time::{Duration, Instant};

use windows::Win32::Foundation::{CloseHandle, HWND, LPARAM, WPARAM};
use windows::Win32::System::Threading::{
    INFINITE, OpenProcess, PROCESS_SYNCHRONIZE, WaitForSingleObject,
};
use windows::Win32::UI::Shell::{NOTIFYICONIDENTIFIER, Shell_NotifyIconGetRect};
use windows::Win32::UI::WindowsAndMessaging::{
    FindWindowW, GetWindowThreadProcessId, PostMessageW, RegisterWindowMessageW,
};
use windows::core::{PCWSTR, w};

/// How often a shell is looked for while there is none: often at first (it is being started
/// anew), then seldom (there may be no shell for a long while, as when the user ended it).
const LOOKED_FOR_EVERY: Duration = Duration::from_millis(250);
const LOOKED_FOR_SELDOM: Duration = Duration::from_secs(2);
const SELDOM_AFTER: Duration = Duration::from_secs(30);

/// The process of the shell that has the taskbar now; `None` when there is none.
fn shell_process() -> Option<u32> {
    let taskbar = unsafe { FindWindowW(w!("Shell_TrayWnd"), PCWSTR::null()) }.ok()?;
    let mut process = 0u32;
    unsafe { GetWindowThreadProcessId(taskbar, Some(&mut process)) };
    (process != 0).then_some(process)
}

/// Waits for a process to end. `false` when it could not be waited for (it may be gone already).
fn wait_for_end(process: u32) -> bool {
    let Ok(handle) = (unsafe { OpenProcess(PROCESS_SYNCHRONIZE, false, process) }) else {
        return false;
    };
    unsafe {
        WaitForSingleObject(handle, INFINITE);
        let _ = CloseHandle(handle);
    }
    true
}

/// Waits for a shell other than `gone` to have the taskbar; its process.
fn wait_for_shell(gone: Option<u32>) -> u32 {
    let began = Instant::now();
    loop {
        if let Some(process) = shell_process().filter(|process| Some(*process) != gone) {
            return process;
        }
        std::thread::sleep(if began.elapsed() < SELDOM_AFTER {
            LOOKED_FOR_EVERY
        } else {
            LOOKED_FOR_SELDOM
        });
    }
}

/// Calls `back` every time a shell has the taskbar that had not had it when watching began (one
/// started anew, or the first, when there was none), from a thread of its own that waits on the
/// shell meanwhile (it does nothing until the shell ends).
pub fn watch_shell(back: impl Fn() + Send + 'static) {
    let watching = std::thread::Builder::new()
        .name("shell watch".into())
        .spawn(move || {
            let mut shell = match shell_process() {
                Some(process) => process,
                None => {
                    let process = wait_for_shell(None);
                    back();
                    process
                }
            };
            loop {
                // Seen to end, whatever has the taskbar next is another shell, even under the
                // same number (the system gives the numbers of ended processes out again).
                let gone = if wait_for_end(shell) {
                    None
                } else {
                    // (Not to be waited on: looked at now and then instead, by its number.)
                    std::thread::sleep(LOOKED_FOR_SELDOM);
                    if shell_process() == Some(shell) {
                        continue;
                    }
                    Some(shell)
                };
                shell = wait_for_shell(gone);
                back();
            }
        });
    if let Err(error) = watching {
        eprintln!("the shell cannot be watched: {error}");
    }
}

/// The most numbers an icon of one window is looked for under. (tray-icon numbers its icons as
/// it makes them, and does not say the number: the window has the one.)
const NUMBERS: u32 = 32;

/// Whether the shell has an icon of this window, shown or among those it hides (the system says
/// "there, but not shown" of those).
pub fn icon_there(window: isize) -> bool {
    let window = HWND(window as *mut _);
    (0..NUMBERS).any(|number| {
        let icon = NOTIFYICONIDENTIFIER {
            cbSize: size_of::<NOTIFYICONIDENTIFIER>() as u32,
            hWnd: window,
            uID: number,
            ..Default::default()
        };
        unsafe { Shell_NotifyIconGetRect(&icon) }.is_ok()
    })
}

/// Tells the window of the icon what the shell tells every window as it is back: tray-icon puts
/// its icon back then, as it was (the same number, so the shell shows it where the user had it).
pub fn put_icon_back(window: isize) {
    let window = HWND(window as *mut _);
    let back = unsafe { RegisterWindowMessageW(w!("TaskbarCreated")) };
    if back == 0 {
        return;
    }
    let _ = unsafe { PostMessageW(Some(window), back, WPARAM(0), LPARAM(0)) };
}
