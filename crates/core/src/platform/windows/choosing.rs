//! The system's window to choose a file to open: here, an image (its kinds listed first, every file
//! after them, as the Tauri version's choice had it).

use std::ffi::OsString;
use std::os::windows::ffi::OsStringExt;
use std::path::PathBuf;

use windows::Win32::Foundation::{ERROR_CANCELLED, HWND};
use windows::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx,
    CoUninitialize,
};
use windows::Win32::UI::Shell::Common::COMDLG_FILTERSPEC;
use windows::Win32::UI::Shell::{
    FOS_FILEMUSTEXIST, FOS_FORCEFILESYSTEM, FileOpenDialog, IFileOpenDialog, SIGDN_FILESYSPATH,
};
use windows::core::{HRESULT, HSTRING, PCWSTR};

/// Lets the user choose an image file, over `owner` (which cannot be used meanwhile). `images` and
/// `all` name the two lists of files. Waits for the answer: not to be called where the windows are
/// drawn. `None` when the window was closed without a choice.
pub fn choose_image(
    owner: Option<isize>,
    kinds: &[&str],
    images: &str,
    all: &str,
) -> Result<Option<PathBuf>, String> {
    // (A thread of its own: the system's window wants one of a single apartment.)
    let started = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) };
    let chosen = choose(owner, kinds, images, all).map_err(|error| error.message());
    if started.is_ok() {
        unsafe { CoUninitialize() };
    }
    chosen
}

fn choose(
    owner: Option<isize>,
    kinds: &[&str],
    images: &str,
    all: &str,
) -> windows::core::Result<Option<PathBuf>> {
    let dialog: IFileOpenDialog =
        unsafe { CoCreateInstance(&FileOpenDialog, None, CLSCTX_INPROC_SERVER)? };
    let pattern = HSTRING::from(
        kinds
            .iter()
            .map(|kind| format!("*{kind}"))
            .collect::<Vec<_>>()
            .join(";"),
    );
    let (images, all, every) = (
        HSTRING::from(images),
        HSTRING::from(all),
        HSTRING::from("*.*"),
    );
    let lists = [
        COMDLG_FILTERSPEC {
            pszName: PCWSTR(images.as_ptr()),
            pszSpec: PCWSTR(pattern.as_ptr()),
        },
        COMDLG_FILTERSPEC {
            pszName: PCWSTR(all.as_ptr()),
            pszSpec: PCWSTR(every.as_ptr()),
        },
    ];
    unsafe {
        dialog.SetFileTypes(&lists)?;
        dialog.SetFileTypeIndex(1)?;
        dialog.SetOptions(dialog.GetOptions()? | FOS_FILEMUSTEXIST | FOS_FORCEFILESYSTEM)?;
        let owner = owner.map(|owner| HWND(owner as *mut _));
        if let Err(error) = dialog.Show(owner) {
            if error.code() == HRESULT::from_win32(ERROR_CANCELLED.0) {
                return Ok(None);
            }
            return Err(error);
        }
        let item = dialog.GetResult()?;
        let path = item.GetDisplayName(SIGDN_FILESYSPATH)?;
        // (As the system has the name: also one that is not whole Unicode.)
        let chosen = Some(PathBuf::from(OsString::from_wide(path.as_wide())));
        windows::Win32::System::Com::CoTaskMemFree(Some(path.0 as *const _));
        Ok(chosen)
    }
}
