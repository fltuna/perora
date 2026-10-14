//! Windows: a file on the clipboard, as the file manager puts one there when it is copied. Pasted
//! where files are taken (a folder, a chat), it is that file, of the kind it is; where only
//! pictures are taken, it is nothing.

use std::os::windows::ffi::OsStrExt;
use std::path::Path;
use std::time::Duration;

use windows::Win32::Foundation::{GlobalFree, HANDLE};
use windows::Win32::System::DataExchange::{
    CloseClipboard, EmptyClipboard, OpenClipboard, SetClipboardData,
};
use windows::Win32::System::Memory::{GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalUnlock};

/// The clipboard's name for a list of files.
const FILES: u32 = 15;
/// How long what begins such a list is, and where in it "the names are wide letters" is said.
const LIST_STARTS: usize = 20;
const WIDE_AT: usize = 16;

/// How often the clipboard is asked for when another program has it, and how long between.
const ASKED: u32 = 10;
const ASKED_EVERY: Duration = Duration::from_millis(20);

/// Puts one file on the clipboard, in place of whatever is there. Refused with the reason when
/// the clipboard could not be had or would not take it.
pub fn put_file(path: &Path) -> Result<(), String> {
    // The start of the list (where the names begin, that they are wide letters), the name with
    // its end, and the end of the list.
    let name: Vec<u16> = path.as_os_str().encode_wide().chain([0, 0]).collect();
    let mut list = vec![0u8; LIST_STARTS];
    list[..4].copy_from_slice(&(LIST_STARTS as u32).to_le_bytes());
    list[WIDE_AT..WIDE_AT + 4].copy_from_slice(&1u32.to_le_bytes());
    list.extend(name.iter().flat_map(|unit| unit.to_le_bytes()));

    unsafe {
        let memory = GlobalAlloc(GMEM_MOVEABLE, list.len()).map_err(|error| error.message())?;
        let there = GlobalLock(memory);
        if there.is_null() {
            let _ = GlobalFree(Some(memory));
            return Err("no room for what goes on the clipboard".to_owned());
        }
        std::ptr::copy_nonoverlapping(list.as_ptr(), there.cast(), list.len());
        let _ = GlobalUnlock(memory);

        let mut opened = OpenClipboard(None);
        for _ in 1..ASKED {
            if opened.is_ok() {
                break;
            }
            std::thread::sleep(ASKED_EVERY);
            opened = OpenClipboard(None);
        }
        if let Err(error) = opened {
            let _ = GlobalFree(Some(memory));
            return Err(error.message());
        }
        let put = EmptyClipboard().and_then(|()| SetClipboardData(FILES, Some(HANDLE(memory.0))));
        let _ = CloseClipboard();
        // (Taken, the memory is the clipboard's from then on.)
        put.map(|_| ()).map_err(|error| {
            let _ = GlobalFree(Some(memory));
            error.message()
        })
    }
}
