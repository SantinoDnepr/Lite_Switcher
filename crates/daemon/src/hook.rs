//! Low-level keyboard / mouse hooks, the typed-word tracking and the word conversion.

use std::cell::RefCell;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

use windows::Win32::Foundation::{HINSTANCE, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetKeyState, VIRTUAL_KEY, VK_BACK, VK_CAPITAL, VK_CONTROL, VK_ESCAPE, VK_LCONTROL, VK_LMENU,
    VK_LSHIFT, VK_LWIN, VK_MENU, VK_NUMLOCK, VK_RCONTROL, VK_RETURN, VK_RMENU, VK_RSHIFT, VK_RWIN,
    VK_SCROLL, VK_SHIFT, VK_SPACE, VK_TAB,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, PostThreadMessageW, SetWindowsHookExW, UnhookWindowsHookEx, HHOOK, KBDLLHOOKSTRUCT,
    LLKHF_EXTENDED, WH_KEYBOARD_LL, WH_MOUSE_LL, WM_APP, WM_KEYDOWN, WM_KEYUP, WM_LBUTTONDOWN,
    WM_MBUTTONDOWN, WM_RBUTTONDOWN, WM_SYSKEYDOWN, WM_SYSKEYUP,
};

use lite_switcher_core::config::Config;
use lite_switcher_core::layout::{self, key_to_char};
use lite_switcher_core::word::{KeyStroke, WordBuffer};

use crate::inject::{self, MARKER};

/// Posted to the main thread when the conversion hotkey is pressed.
pub const WM_CONVERT: u32 = WM_APP + 1;
/// Posted to the main thread when the pause state changed (the tray has to be refreshed).
pub const WM_TRAY_REFRESH: u32 = WM_APP + 2;

static MAIN_THREAD: AtomicU32 = AtomicU32::new(0);
pub static PAUSED: AtomicBool = AtomicBool::new(false);

struct State {
    config: Config,
    buffer: WordBuffer,
    /// Window the buffer belongs to.
    buffer_hwnd: isize,
    /// Cached "is the foreground app ignored" answer for `ignored_hwnd`.
    ignored_hwnd: isize,
    ignored: bool,
    hotkey_down: bool,
    swallow_up: bool,
}

thread_local! {
    static STATE: RefCell<Option<State>> = const { RefCell::new(None) };
}

fn with_state<R>(f: impl FnOnce(&mut State) -> R) -> Option<R> {
    STATE.with(|s| s.try_borrow_mut().ok().and_then(|mut s| s.as_mut().map(f)))
}

pub fn set_config(config: Config) {
    STATE.with(|s| {
        let mut slot = s.borrow_mut();
        match slot.as_mut() {
            Some(state) => {
                state.config = config;
                state.ignored_hwnd = 0;
            }
            None => {
                *slot = Some(State {
                    config,
                    buffer: WordBuffer::default(),
                    buffer_hwnd: 0,
                    ignored_hwnd: 0,
                    ignored: false,
                    hotkey_down: false,
                    swallow_up: false,
                })
            }
        }
    });
}

/// Posts a thread message to the main (hook) thread.
pub fn post_to_main(msg: u32) {
    unsafe {
        let _ = PostThreadMessageW(MAIN_THREAD.load(Ordering::Relaxed), msg, WPARAM(0), LPARAM(0));
    }
}

pub struct Hooks(HHOOK, HHOOK);

impl Drop for Hooks {
    fn drop(&mut self) {
        unsafe {
            let _ = UnhookWindowsHookEx(self.0);
            let _ = UnhookWindowsHookEx(self.1);
        }
    }
}

/// Installs the hooks in the calling thread, which must run a message loop.
pub fn install() -> windows::core::Result<Hooks> {
    unsafe {
        MAIN_THREAD.store(GetCurrentThreadId(), Ordering::Relaxed);
        let module = GetModuleHandleW(None)?;
        let hinst = Some(HINSTANCE(module.0));
        let kb = SetWindowsHookExW(WH_KEYBOARD_LL, Some(keyboard_proc), hinst, 0)?;
        let mouse = SetWindowsHookExW(WH_MOUSE_LL, Some(mouse_proc), hinst, 0)?;
        Ok(Hooks(kb, mouse))
    }
}

unsafe extern "system" fn keyboard_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code == 0 {
        let kb = &*(lparam.0 as *const KBDLLHOOKSTRUCT);
        if kb.dwExtraInfo != MARKER {
            let swallow = catch_unwind(AssertUnwindSafe(|| on_key(wparam.0 as u32, kb))).unwrap_or(false);
            if swallow {
                return LRESULT(1);
            }
        }
    }
    CallNextHookEx(None, code, wparam, lparam)
}

unsafe extern "system" fn mouse_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code == 0 && matches!(wparam.0 as u32, WM_LBUTTONDOWN | WM_RBUTTONDOWN | WM_MBUTTONDOWN) {
        with_state(|s| s.buffer.clear());
    }
    CallNextHookEx(None, code, wparam, lparam)
}

fn modifiers_match(hk: &lite_switcher_core::hotkey::Hotkey) -> bool {
    inject::is_down(VK_CONTROL) == hk.ctrl
        && inject::is_down(VK_MENU) == hk.alt
        && inject::is_down(VK_SHIFT) == hk.shift
        && (inject::is_down(VK_LWIN) || inject::is_down(VK_RWIN)) == hk.win
}

fn is_modifier(vk: VIRTUAL_KEY) -> bool {
    matches!(
        vk,
        VK_SHIFT | VK_LSHIFT | VK_RSHIFT | VK_CONTROL | VK_LCONTROL | VK_RCONTROL | VK_MENU | VK_LMENU
            | VK_RMENU | VK_LWIN | VK_RWIN | VK_CAPITAL | VK_NUMLOCK | VK_SCROLL
    )
}

/// Returns true if the key event must be swallowed.
fn on_key(msg: u32, kb: &KBDLLHOOKSTRUCT) -> bool {
    let down = matches!(msg, WM_KEYDOWN | WM_SYSKEYDOWN);
    let up = matches!(msg, WM_KEYUP | WM_SYSKEYUP);
    let vk = VIRTUAL_KEY(kb.vkCode as u16);
    let paused = PAUSED.load(Ordering::Relaxed);

    with_state(|s| {
        let hotkey = s.config.hotkeys.convert_word;

        if vk.0 as u32 == hotkey.vk {
            if up {
                s.hotkey_down = false;
                if std::mem::take(&mut s.swallow_up) {
                    return true;
                }
            } else if down && !paused && modifiers_match(&hotkey) && !foreground_ignored(s) {
                if !s.hotkey_down {
                    s.hotkey_down = true;
                    s.swallow_up = true;
                    post_to_main(WM_CONVERT);
                }
                return true; // also swallows auto-repeat
            }
        }

        if !down || paused || is_modifier(vk) {
            return false;
        }
        if foreground_ignored(s) {
            return false;
        }
        track_key(s, vk, kb);
        false
    })
    .unwrap_or(false)
}

/// Is the foreground application in `ignored_apps`? Also resets the buffer when the window changes.
fn foreground_ignored(s: &mut State) -> bool {
    let hwnd = inject::foreground();
    let raw = hwnd.0 as isize;
    if s.buffer_hwnd != raw {
        s.buffer.clear();
        s.buffer_hwnd = raw;
    }
    if s.ignored_hwnd != raw {
        s.ignored_hwnd = raw;
        s.ignored = inject::window_exe(hwnd).is_some_and(|exe| {
            s.config.troubleshooting.ignored_apps.iter().any(|a| a.eq_ignore_ascii_case(&exe))
        });
    }
    s.ignored
}

fn track_key(s: &mut State, vk: VIRTUAL_KEY, kb: &KBDLLHOOKSTRUCT) {
    let ctrl_alt_win = inject::is_down(VK_CONTROL)
        || inject::is_down(VK_MENU)
        || inject::is_down(VK_LWIN)
        || inject::is_down(VK_RWIN);
    if ctrl_alt_win {
        s.buffer.clear();
        return;
    }
    match vk {
        VK_BACK => s.buffer.backspace(),
        VK_SPACE => s.buffer.space(s.config.common.keep_trailing_space),
        VK_RETURN | VK_TAB | VK_ESCAPE => s.buffer.clear(),
        // numpad keys and extended keys (arrows, Home, End, Delete...) are not part of a word
        _ if (0x60..=0x6F).contains(&vk.0) || kb.flags.0 & LLKHF_EXTENDED.0 != 0 => s.buffer.clear(),
        _ => {
            let hkl = inject::window_hkl(inject::foreground());
            let shift = inject::is_down(VK_SHIFT);
            let caps = unsafe { GetKeyState(VK_CAPITAL.0 as i32) } & 1 != 0;
            let scancode = kb.scanCode as u16;
            if key_to_char(hkl, scancode, shift, caps).is_some() {
                s.buffer.push(KeyStroke { scancode, shift, caps });
            } else {
                s.buffer.clear();
            }
        }
    }
}

/// Converts the last typed word (called from the main thread on `WM_CONVERT`).
pub fn convert_word() {
    let hwnd = inject::foreground();
    let plan = with_state(|s| {
        if hwnd.0 as isize != s.buffer_hwnd || s.buffer.is_empty() {
            return None;
        }
        let layouts = layout::resolve_layouts(&layout::enumerate(), &s.config.common);
        let origin = layout::describe(inject::window_hkl(hwnd)).id;
        let replacement = s.buffer.next_replacement(
            || layout::cycle_sequence(&layouts, Some(&origin)),
            |k, l| key_to_char(l.hkl, k.scancode, k.shift, k.caps),
        )?;
        Some((replacement, s.config.clone()))
    })
    .flatten();

    // The state is not borrowed while the events are sent: the hook sees (and ignores) them.
    let Some((replacement, config)) = plan else { return };
    let delay = config.troubleshooting.key_delay_ms;
    tracing::debug!(
        "convert: erase {}, type {} chars, layout {}",
        replacement.backspaces,
        replacement.text.chars().count(),
        replacement.target.id
    );
    inject::release_hotkey_modifiers(&config.hotkeys.convert_word);
    inject::send_backspaces(replacement.backspaces, delay);
    inject::switch_layout(hwnd, replacement.target.hkl);
    inject::send_text(&replacement.text, delay);
    if config.sounds.enabled {
        inject::play_sound(&config.sounds.on_convert);
    }
}
