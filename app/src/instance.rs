//! One Perora at a time: a second one would spend the first one's sign-in (its refresh token is used
//! once). Started again, it hands its arguments to the one running and ends.
//!
//! Done as the Tauri version's plugin did (tauri-plugin-single-instance 2.5.3), with its names and its
//! message, so that nothing about it changes for the user: a named mutex `dev.f2a.perora-sim`, a
//! message window of class `dev.f2a.perora-sic` named `dev.f2a.perora-siw`, and `WM_COPYDATA` with
//! `dwData` 1542 carrying `cwd|arg0|arg1|…` in UTF-8, ended by a NUL.

use std::cell::RefCell;

use windows::Win32::Foundation::{
    ERROR_ALREADY_EXISTS, GetLastError, HANDLE, HWND, LPARAM, LRESULT, WPARAM,
};
use windows::Win32::System::DataExchange::COPYDATASTRUCT;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Threading::CreateMutexW;
use windows::Win32::UI::WindowsAndMessaging::{
    AllowSetForegroundWindow, CreateWindowExW, DefWindowProcW, FindWindowW,
    GetWindowThreadProcessId, HWND_MESSAGE, RegisterClassW, SMTO_NORMAL, SendMessageTimeoutW,
    WINDOW_EX_STYLE, WINDOW_STYLE, WM_COPYDATA, WNDCLASSW,
};
use windows::core::{HSTRING, PCWSTR};

const MUTEX: &str = "dev.f2a.perora-sim";
const CLASS: &str = "dev.f2a.perora-sic";
const NAME: &str = "dev.f2a.perora-siw";
const DATA: usize = 1542;
/// How long a second Perora waits for the first one to take its arguments.
const FORWARD_TIMEOUT_MS: u32 = 10_000;

/// What a second start told the one running: its folder, and its arguments (its name first).
pub struct Forwarded {
    pub folder: String,
    pub arguments: Vec<String>,
}

/// What a later start's arguments are handed to.
type Receiver = Box<dyn Fn(Forwarded)>;

thread_local! {
    static RECEIVER: RefCell<Option<Receiver>> = const { RefCell::new(None) };
}

/// Kept while the application runs: the mutex says it is running.
pub struct Instance {
    _mutex: HANDLE,
}

/// The first Perora gets `Some`; a second one has handed its arguments over by the time it gets
/// `None`, and should end.
pub fn claim() -> Option<Instance> {
    let mutex = unsafe { CreateMutexW(None, true, &HSTRING::from(MUTEX)) };
    let already = unsafe { GetLastError() } == ERROR_ALREADY_EXISTS;
    let mutex = mutex.ok()?;
    if !already {
        return Some(Instance { _mutex: mutex });
    }
    unsafe {
        let Ok(window) = FindWindowW(&HSTRING::from(CLASS), &HSTRING::from(NAME)) else {
            // Running, but without its window yet (starting): this start is lost, as in the plugin.
            return None;
        };
        // The one running may bring its window to the front for this start; only the one in front
        // (this one, just started) can let it.
        let mut pid = 0;
        GetWindowThreadProcessId(window, Some(&mut pid));
        if pid != 0 {
            let _ = AllowSetForegroundWindow(pid);
        }
        let folder = std::env::current_dir().unwrap_or_default();
        let arguments: Vec<String> = std::env::args_os()
            .map(|argument| argument.to_string_lossy().into_owned())
            .collect();
        let data = format!("{}|{}\0", folder.to_string_lossy(), arguments.join("|"));
        let copy = COPYDATASTRUCT {
            dwData: DATA,
            cbData: data.len() as u32,
            lpData: data.as_ptr() as *mut _,
        };
        let sent = SendMessageTimeoutW(
            window,
            WM_COPYDATA,
            WPARAM(0),
            LPARAM(&copy as *const _ as isize),
            SMTO_NORMAL,
            FORWARD_TIMEOUT_MS,
            None,
        );
        if sent.0 == 0 {
            eprintln!("the Perora that is running did not take this start's arguments");
        }
    }
    None
}

/// Makes the window that second starts send their arguments to, and calls `receive` with them (on
/// this thread, from the message loop).
pub fn listen(receive: impl Fn(Forwarded) + 'static) {
    RECEIVER.with(|receiver| *receiver.borrow_mut() = Some(Box::new(receive)));
    unsafe {
        let Ok(module) = GetModuleHandleW(None) else {
            return;
        };
        let class = HSTRING::from(CLASS);
        let description = WNDCLASSW {
            lpfnWndProc: Some(window_procedure),
            hInstance: module.into(),
            lpszClassName: PCWSTR(class.as_ptr()),
            ..Default::default()
        };
        RegisterClassW(&description);
        // A message-only window: never seen, found by its class and name.
        if CreateWindowExW(
            WINDOW_EX_STYLE(0),
            &class,
            &HSTRING::from(NAME),
            WINDOW_STYLE(0),
            0,
            0,
            0,
            0,
            Some(HWND_MESSAGE),
            None,
            Some(module.into()),
            None,
        )
        .is_err()
        {
            eprintln!("the window for later starts could not be made");
        }
    }
}

unsafe extern "system" fn window_procedure(
    window: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if message == WM_COPYDATA {
        let copy = unsafe { &*(lparam.0 as *const COPYDATASTRUCT) };
        if copy.dwData == DATA && !copy.lpData.is_null() {
            let bytes = unsafe {
                std::slice::from_raw_parts(copy.lpData as *const u8, copy.cbData as usize)
            };
            let text = String::from_utf8_lossy(bytes);
            let text = text.trim_end_matches('\0');
            let mut parts = text.split('|').map(str::to_owned);
            let folder = parts.next().unwrap_or_default();
            let forwarded = Forwarded {
                folder,
                arguments: parts.collect(),
            };
            RECEIVER.with(|receiver| {
                if let Some(receive) = receiver.borrow().as_ref() {
                    receive(forwarded);
                }
            });
            return LRESULT(1);
        }
    }
    unsafe { DefWindowProcW(window, message, wparam, lparam) }
}
