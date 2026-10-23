//! Clipboard access: text read / write and a snapshot that lets us put the user's data back.

use std::ptr;
use std::thread::sleep;
use std::time::Duration;

use windows::Win32::Foundation::{HANDLE, HGLOBAL};
use windows::Win32::System::DataExchange::{
    CloseClipboard, EmptyClipboard, EnumClipboardFormats, GetClipboardData, GetClipboardSequenceNumber,
    OpenClipboard, SetClipboardData,
};
use windows::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalSize, GlobalUnlock, GMEM_MOVEABLE};

const CF_UNICODETEXT: u32 = 13;

/// Formats of the clipboard: (format id, raw bytes).
pub type Snapshot = Vec<(u32, Vec<u8>)>;

/// Keeps the clipboard open; closes it when dropped.
struct Open;

impl Drop for Open {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseClipboard();
        }
    }
}

/// Another program may hold the clipboard for a moment, so retry for a while.
fn open() -> Option<Open> {
    for _ in 0..30 {
        if unsafe { OpenClipboard(None) }.is_ok() {
            return Some(Open);
        }
        sleep(Duration::from_millis(10));
    }
    None
}

/// Changes whenever the clipboard content changes.
pub fn sequence() -> u32 {
    unsafe { GetClipboardSequenceNumber() }
}

/// Formats whose data is a GDI handle or owner-drawn: they cannot be copied as bytes.
fn skip_format(format: u32) -> bool {
    matches!(format, 2 | 3 | 9 | 14 | 0x80..=0x8E) || (0x300..=0x3FF).contains(&format)
}

unsafe fn read_global(handle: HGLOBAL) -> Option<Vec<u8>> {
    if handle.0.is_null() {
        return None;
    }
    let size = GlobalSize(handle);
    if size == 0 {
        return None;
    }
    let data = GlobalLock(handle);
    if data.is_null() {
        return None;
    }
    let bytes = std::slice::from_raw_parts(data as *const u8, size).to_vec();
    let _ = GlobalUnlock(handle);
    Some(bytes)
}

unsafe fn set_bytes(format: u32, bytes: &[u8]) -> bool {
    let Ok(handle) = GlobalAlloc(GMEM_MOVEABLE, bytes.len().max(1)) else { return false };
    let data = GlobalLock(handle);
    if data.is_null() {
        return false;
    }
    ptr::copy_nonoverlapping(bytes.as_ptr(), data as *mut u8, bytes.len());
    let _ = GlobalUnlock(handle);
    SetClipboardData(format, Some(HANDLE(handle.0))).is_ok()
}

/// Copies all formats that can be copied as bytes. `None` if the clipboard cannot be opened.
pub fn snapshot() -> Option<Snapshot> {
    let _open = open()?;
    let mut formats = Vec::new();
    unsafe {
        let mut format = 0;
        loop {
            format = EnumClipboardFormats(format);
            if format == 0 {
                break;
            }
            if skip_format(format) {
                continue;
            }
            if let Some(bytes) = GetClipboardData(format).ok().and_then(|h| read_global(HGLOBAL(h.0))) {
                formats.push((format, bytes));
            }
        }
    }
    Some(formats)
}

/// Puts the snapshot back, replacing the current content.
pub fn restore(snapshot: &Snapshot) -> bool {
    let Some(_open) = open() else { return false };
    unsafe {
        let _ = EmptyClipboard();
        snapshot.iter().fold(true, |ok, (format, bytes)| set_bytes(*format, bytes) && ok)
    }
}

pub fn get_text() -> Option<String> {
    let _open = open()?;
    unsafe {
        let handle = GetClipboardData(CF_UNICODETEXT).ok()?;
        let bytes = read_global(HGLOBAL(handle.0))?;
        let units: Vec<u16> = bytes
            .as_chunks::<2>()
            .0
            .iter()
            .map(|c| u16::from_le_bytes(*c))
            .take_while(|&u| u != 0)
            .collect();
        Some(String::from_utf16_lossy(&units))
    }
}

pub fn set_text(text: &str) -> bool {
    let Some(_open) = open() else { return false };
    let bytes: Vec<u8> = text.encode_utf16().chain(std::iter::once(0)).flat_map(u16::to_le_bytes).collect();
    unsafe {
        let _ = EmptyClipboard();
        set_bytes(CF_UNICODETEXT, &bytes)
    }
}
