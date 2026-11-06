//! One Perora at a time: a second one would spend the first one's sign-in (its refresh token is used
//! once). Started again, it hands its arguments to the one running and ends.
//!
//! Done as the Tauri version's plugin did (tauri-plugin-single-instance 2.5.3), with its names and its
//! message, so that nothing about it changes for the user: a named mutex `dev.f2a.perora-sim`, a
//! message window of class `dev.f2a.perora-sic` named `dev.f2a.perora-siw`, and `WM_COPYDATA` with
//! `dwData` 1542 carrying `cwd|arg0|arg1|…` in UTF-8, ended by a NUL.

use std::cell::RefCell;

use windows::Win32::Foundation::{
    CloseHandle, ERROR_ALREADY_EXISTS, GetLastError, HANDLE, HWND, LPARAM, LRESULT, WPARAM,
};
use windows::Win32::System::DataExchange::COPYDATASTRUCT;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Threading::{CreateMutexW, ReleaseMutex};
use windows::Win32::UI::WindowsAndMessaging::{
    AllowSetForegroundWindow, CreateWindowExW, DefWindowProcW, DestroyWindow, FindWindowW,
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
    /// What later starts sent before anyone listened (the application was still starting).
    static WAITING: RefCell<Vec<Forwarded>> = const { RefCell::new(Vec::new()) };
}

/// Kept while the application runs: the mutex says it is running, the window takes later starts.
pub struct Instance {
    mutex: HANDLE,
    window: Option<HWND>,
}

impl Drop for Instance {
    fn drop(&mut self) {
        unsafe {
            if let Some(window) = self.window.take() {
                let _ = DestroyWindow(window);
            }
            let _ = ReleaseMutex(self.mutex);
            let _ = CloseHandle(self.mutex);
        }
    }
}

/// How long a second Perora looks for the first one's window, when the first one has only just
/// started and has not made it yet (it makes it at once, before anything else).
const WINDOW_APPEARS_WITHIN_MS: u64 = 3_000;

/// The first Perora gets `Some`, with the window later starts send their arguments to already
/// made (they are kept until `listen`); a second one has handed its arguments over by the time it
/// gets `None`, and should end.
pub fn claim() -> Option<Instance> {
    let mutex = unsafe { CreateMutexW(None, true, &HSTRING::from(MUTEX)) };
    let already = unsafe { GetLastError() } == ERROR_ALREADY_EXISTS;
    let mutex = mutex.ok()?;
    if !already {
        let window = make_window();
        return Some(Instance { mutex, window });
    }
    unsafe {
        let _ = CloseHandle(mutex);
        // The first one makes its window right after the mutex, but it may be between the two.
        let began = std::time::Instant::now();
        let window = loop {
            if let Ok(window) = FindWindowW(&HSTRING::from(CLASS), &HSTRING::from(NAME)) {
                break window;
            }
            if began.elapsed().as_millis() as u64 >= WINDOW_APPEARS_WITHIN_MS {
                eprintln!("the Perora that is running has no window for this start's arguments");
                return None;
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
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

/// Calls `receive` with what later starts send (on this thread, from the message loop), first with
/// what they sent before this.
pub fn listen(receive: impl Fn(Forwarded) + 'static) {
    for forwarded in WAITING.with(|waiting| std::mem::take(&mut *waiting.borrow_mut())) {
        receive(forwarded);
    }
    RECEIVER.with(|receiver| *receiver.borrow_mut() = Some(Box::new(receive)));
}

/// The window that later starts send their arguments to: a message-only window, never seen, found by
/// its class and name. Made on the thread that runs the message loop (the main one).
fn make_window() -> Option<HWND> {
    unsafe {
        let module = GetModuleHandleW(None).ok()?;
        let class = HSTRING::from(CLASS);
        let description = WNDCLASSW {
            lpfnWndProc: Some(window_procedure),
            hInstance: module.into(),
            lpszClassName: PCWSTR(class.as_ptr()),
            ..Default::default()
        };
        RegisterClassW(&description);
        let made = CreateWindowExW(
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
        );
        if made.is_err() {
            eprintln!("the window for later starts could not be made");
        }
        made.ok()
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
            RECEIVER.with(|receiver| match receiver.borrow().as_ref() {
                Some(receive) => receive(forwarded),
                None => WAITING.with(|waiting| waiting.borrow_mut().push(forwarded)),
            });
            return LRESULT(1);
        }
    }
    unsafe { DefWindowProcW(window, message, wparam, lparam) }
}
