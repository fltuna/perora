//! Windows: what Perora puts on the clipboard.
//!
//! - A picture: as a PNG first (under the name "PNG", which programs that keep transparency ask
//!   for), then as a bitmap (`CF_DIBV5`, which every program that takes pictures reads, and from
//!   which the system makes the older kinds). That is the order the Tauri version's clipboard
//!   library (arboard) wrote them in: some programs take the first kind they know.
//! - A file, as the file manager puts one there when it is copied. Pasted where files are taken
//!   (a folder, a chat), it is that file, of the kind it is; where only pictures are taken, it is
//!   nothing.

use std::os::windows::ffi::OsStrExt;
use std::path::Path;
use std::time::Duration;

use windows::Win32::Foundation::{GlobalFree, HANDLE, HGLOBAL};
use windows::Win32::System::DataExchange::{
    CloseClipboard, EmptyClipboard, OpenClipboard, RegisterClipboardFormatW, SetClipboardData,
};
use windows::Win32::System::Memory::{GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalUnlock};
use windows::core::w;

use crate::platform::Frame;

/// The clipboard's name for a list of files.
const FILES: u32 = 15;
/// How long what begins such a list is, and where in it "the names are wide letters" is said.
const LIST_STARTS: usize = 20;
const WIDE_AT: usize = 16;

/// The clipboard's name for a bitmap with a header that says how its colours and transparency
/// are laid out.
const BITMAP: u32 = 17;

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
    put(&[(FILES, &list)])
}

/// Puts a picture on the clipboard, in place of whatever is there: the PNG of it (made already)
/// and its pixels as a bitmap. Refused with the reason when the clipboard could not be had or
/// would not take it.
pub fn put_picture(frame: &Frame, png: &[u8]) -> Result<(), String> {
    let png_format = unsafe { RegisterClipboardFormatW(w!("PNG")) };
    if png_format == 0 {
        return Err(windows::core::Error::from_thread().message());
    }
    put(&[(png_format, png), (BITMAP, &bitmap(frame))])
}

/// The pixels with the header of `CF_DIBV5` (`BITMAPV5HEADER`): rows from the bottom, blue first,
/// and the fourth byte of each pixel said to be its transparency (not premultiplied).
fn bitmap(frame: &Frame) -> Vec<u8> {
    const HEADER: u32 = 124;
    const BITFIELDS: u32 = 3;
    const SRGB: u32 = 0x7352_4742; // 'sRGB'
    const IMAGES: u32 = 4; // LCS_GM_IMAGES
    let (width, height) = (frame.area.width, frame.area.height);
    let size = width as usize * height as usize * 4;

    let mut bitmap = Vec::with_capacity(HEADER as usize + size);
    bitmap.extend_from_slice(&HEADER.to_le_bytes());
    bitmap.extend_from_slice(&(width as i32).to_le_bytes());
    // Positive: rows from the bottom.
    bitmap.extend_from_slice(&(height as i32).to_le_bytes());
    bitmap.extend_from_slice(&1u16.to_le_bytes());
    bitmap.extend_from_slice(&32u16.to_le_bytes());
    bitmap.extend_from_slice(&BITFIELDS.to_le_bytes());
    bitmap.extend_from_slice(&(size as u32).to_le_bytes());
    // (No resolution, no palette.)
    bitmap.extend_from_slice(&[0; 16]);
    for mask in [0x00ff_0000u32, 0x0000_ff00, 0x0000_00ff, 0xff00_0000] {
        bitmap.extend_from_slice(&mask.to_le_bytes());
    }
    bitmap.extend_from_slice(&SRGB.to_le_bytes());
    // (The end points of the colour space and its gamma: not used with sRGB.)
    bitmap.extend_from_slice(&[0; 36 + 12]);
    bitmap.extend_from_slice(&IMAGES.to_le_bytes());
    // (No profile.)
    bitmap.extend_from_slice(&[0; 12]);
    debug_assert_eq!(bitmap.len(), HEADER as usize);

    let row = width as usize * 4;
    for line in frame.rgba.chunks_exact(row.max(1)).rev() {
        let start = bitmap.len();
        bitmap.extend_from_slice(line);
        for pixel in bitmap[start..].chunks_exact_mut(4) {
            pixel.swap(0, 2);
        }
    }
    bitmap
}

/// Puts these on the clipboard, each under its format, in place of whatever is there: all of them
/// or (refused) none.
fn put(entries: &[(u32, &[u8])]) -> Result<(), String> {
    unsafe {
        let mut memory: Vec<HGLOBAL> = Vec::with_capacity(entries.len());
        let free = |memory: &[HGLOBAL]| {
            for &one in memory {
                let _ = GlobalFree(Some(one));
            }
        };
        for (_, contents) in entries {
            let one = match GlobalAlloc(GMEM_MOVEABLE, contents.len()) {
                Ok(one) => one,
                Err(error) => {
                    free(&memory);
                    return Err(error.message());
                }
            };
            let there = GlobalLock(one);
            if there.is_null() {
                let _ = GlobalFree(Some(one));
                free(&memory);
                return Err("no room for what goes on the clipboard".to_owned());
            }
            std::ptr::copy_nonoverlapping(contents.as_ptr(), there.cast(), contents.len());
            let _ = GlobalUnlock(one);
            memory.push(one);
        }

        let mut opened = OpenClipboard(None);
        for _ in 1..ASKED {
            if opened.is_ok() {
                break;
            }
            std::thread::sleep(ASKED_EVERY);
            opened = OpenClipboard(None);
        }
        if let Err(error) = opened {
            free(&memory);
            return Err(error.message());
        }
        if let Err(error) = EmptyClipboard() {
            let _ = CloseClipboard();
            free(&memory);
            return Err(error.message());
        }
        for (at, ((format, _), &one)) in entries.iter().zip(&memory).enumerate() {
            if let Err(error) = SetClipboardData(*format, Some(HANDLE(one.0))) {
                // (What was put there is the clipboard's; the rest is still this program's.) Not
                // half of a picture: the clipboard is left empty.
                let _ = EmptyClipboard();
                let _ = CloseClipboard();
                free(&memory[at..]);
                return Err(error.message());
            }
        }
        let _ = CloseClipboard();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geometry::Rect;

    #[test]
    fn a_bitmap_has_the_v5_header_its_rows_from_the_bottom_and_blue_first() {
        // Two pixels across, two down: red and green on top, blue and see-through below.
        let frame = Frame {
            area: Rect::new(0, 0, 2, 2),
            rgba: vec![
                255, 0, 0, 255, 0, 255, 0, 255, //
                0, 0, 255, 255, 1, 2, 3, 0,
            ],
        };
        let bitmap = bitmap(&frame);
        assert_eq!(bitmap.len(), 124 + 16);
        let word = |at: usize| u32::from_le_bytes(bitmap[at..at + 4].try_into().unwrap());
        assert_eq!(word(0), 124);
        assert_eq!(word(4), 2);
        assert_eq!(word(8), 2);
        assert_eq!(word(16), 3, "the masks say where each colour is");
        assert_eq!(word(52), 0xff00_0000, "the fourth byte is the transparency");
        // The bottom row first: blue, then the see-through one as it was.
        assert_eq!(&bitmap[124..132], &[255, 0, 0, 255, 3, 2, 1, 0]);
        // Then the top row: red, green.
        assert_eq!(&bitmap[132..140], &[0, 0, 255, 255, 0, 255, 0, 255]);
    }
}
