//! Synthetic keyboard input and small window helpers.

use std::mem::size_of;
use std::thread::sleep;
use std::time::Duration;

use windows::core::{w, PWSTR};
use windows::Win32::Foundation::{CloseHandle, HWND, LPARAM, WPARAM};
use windows::Win32::Media::Audio::{PlaySoundW, SND_ALIAS, SND_ASYNC, SND_FILENAME, SND_NODEFAULT};
use windows::Win32::System::Threading::{
    OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, GetKeyboardLayout, SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT,
    KEYBD_EVENT_FLAGS, KEYEVENTF_KEYUP, KEYEVENTF_UNICODE, VIRTUAL_KEY, VK_BACK, VK_CONTROL, VK_LWIN,
    VK_MENU, VK_RWIN, VK_SHIFT,
};
use windows::Win32::UI::WindowsAndMessaging::{
    GetForegroundWindow, GetWindowThreadProcessId, PostMessageW, WM_INPUTLANGCHANGEREQUEST,
};

use lite_switcher_core::hotkey::Hotkey;

/// Marks the events sent by this program so the keyboard hook ignores them.
pub const MARKER: usize = 0x4C53_5754;

pub fn is_down(vk: VIRTUAL_KEY) -> bool {
    unsafe { GetAsyncKeyState(vk.0 as i32) as u16 & 0x8000 != 0 }
}

pub fn foreground() -> HWND {
    unsafe { GetForegroundWindow() }
}

/// HKL (raw) active in the given window's input thread.
pub fn window_hkl(hwnd: HWND) -> isize {
    unsafe {
        let tid = GetWindowThreadProcessId(hwnd, None);
        GetKeyboardLayout(tid).0 as isize
    }
}

/// Lower-case executable file name of the window's process, e.g. `notepad.exe`.
pub fn window_exe(hwnd: HWND) -> Option<String> {
    unsafe {
        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let mut buf = [0u16; 520];
        let mut len = buf.len() as u32;
        let ok = QueryFullProcessImageNameW(process, PROCESS_NAME_WIN32, PWSTR(buf.as_mut_ptr()), &mut len);
        let _ = CloseHandle(process);
        ok.ok()?;
        let path = String::from_utf16_lossy(&buf[..len as usize]);
        path.rsplit(['\\', '/']).next().map(|n| n.to_ascii_lowercase())
    }
}

fn key_input(vk: VIRTUAL_KEY, scan: u16, flags: KEYBD_EVENT_FLAGS) -> INPUT {
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT { wVk: vk, wScan: scan, dwFlags: flags, time: 0, dwExtraInfo: MARKER },
        },
    }
}

fn send(inputs: &[INPUT]) {
    unsafe {
        SendInput(inputs, size_of::<INPUT>() as i32);
    }
}

/// Sends the groups of inputs; with a non-zero delay every group is sent separately.
fn send_groups(groups: Vec<Vec<INPUT>>, delay_ms: u32) {
    if delay_ms == 0 {
        send(&groups.concat());
        return;
    }
    for group in groups {
        send(&group);
        sleep(Duration::from_millis(delay_ms as u64));
    }
}

/// Releases the modifiers of the hotkey that are still held, so that e.g. Ctrl+Backspace is not sent.
pub fn release_hotkey_modifiers(hk: &Hotkey) {
    let mut inputs = Vec::new();
    for (wanted, vk) in [(hk.ctrl, VK_CONTROL), (hk.alt, VK_MENU), (hk.shift, VK_SHIFT), (hk.win, VK_LWIN)] {
        if wanted && is_down(vk) {
            inputs.push(key_input(vk, 0, KEYEVENTF_KEYUP));
        }
    }
    if hk.win && is_down(VK_RWIN) {
        inputs.push(key_input(VK_RWIN, 0, KEYEVENTF_KEYUP));
    }
    if !inputs.is_empty() {
        send(&inputs);
    }
}

pub fn send_backspaces(count: usize, delay_ms: u32) {
    let groups = (0..count)
        .map(|_| vec![key_input(VK_BACK, 0, KEYBD_EVENT_FLAGS(0)), key_input(VK_BACK, 0, KEYEVENTF_KEYUP)])
        .collect();
    send_groups(groups, delay_ms);
}

/// Types the text as Unicode characters (independent of the active layout, no clipboard).
pub fn send_text(text: &str, delay_ms: u32) {
    let groups = text
        .encode_utf16()
        .map(|unit| {
            vec![
                key_input(VIRTUAL_KEY(0), unit, KEYEVENTF_UNICODE),
                key_input(VIRTUAL_KEY(0), unit, KEYEVENTF_UNICODE | KEYEVENTF_KEYUP),
            ]
        })
        .collect();
    send_groups(groups, delay_ms);
}

/// Asks the window to switch its input language.
pub fn switch_layout(hwnd: HWND, hkl: isize) {
    unsafe {
        let _ = PostMessageW(Some(hwnd), WM_INPUTLANGCHANGEREQUEST, WPARAM(0), LPARAM(hkl));
    }
}

/// Plays the given .wav file, or the system default sound when the path is empty.
pub fn play_sound(wav: &str) {
    unsafe {
        if wav.is_empty() {
            let _ = PlaySoundW(w!("SystemAsterisk"), None, SND_ALIAS | SND_ASYNC);
        } else {
            let wide: Vec<u16> = wav.encode_utf16().chain(std::iter::once(0)).collect();
            let _ = PlaySoundW(
                windows::core::PCWSTR(wide.as_ptr()),
                None,
                SND_FILENAME | SND_ASYNC | SND_NODEFAULT,
            );
        }
    }
}
