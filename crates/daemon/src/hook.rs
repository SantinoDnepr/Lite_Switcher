//! Low-level keyboard / mouse hooks, the typed-word tracking and the hotkey actions.

use std::cell::RefCell;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::atomic::{AtomicU32, Ordering};

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
    WM_MBUTTONDOWN, WM_QUIT, WM_RBUTTONDOWN, WM_SYSKEYDOWN, WM_SYSKEYUP,
};

use lite_switcher_core::config::{Config, Hotkeys, Mode};
use lite_switcher_core::hotkey::Hotkey;
use lite_switcher_core::layout::{self, key_to_char, Layout};
use lite_switcher_core::word::{KeyStroke, WordBuffer};

use crate::inject::{self, MARKER};
use crate::selection::{self, Kind};

/// Posted to the main thread when a hotkey is pressed; `wParam` is the `Action`.
pub const WM_ACTION: u32 = WM_APP + 1;

static MAIN_THREAD: AtomicU32 = AtomicU32::new(0);

/// What a hotkey (or a tray menu item) asks the daemon to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    ConvertWord,
    ConvertSelection,
    InvertCase,
    /// Hotkey: switches between `manual` and the last automatic mode.
    ToggleAutoswitch,
    /// Tray menu: select a mode.
    SetManual,
    SetAutoLiterary,
    SetAutoProgramming,
}

impl Action {
    pub fn from_wparam(value: usize) -> Option<Action> {
        [
            Action::ConvertWord,
            Action::ConvertSelection,
            Action::InvertCase,
            Action::ToggleAutoswitch,
            Action::SetManual,
            Action::SetAutoLiterary,
            Action::SetAutoProgramming,
        ]
        .into_iter()
        .find(|a| *a as usize == value)
    }

    /// The action of the tray menu item that selects the mode.
    pub fn set_mode(mode: Mode) -> Action {
        match mode {
            Mode::Manual => Action::SetManual,
            Mode::AutoLiterary => Action::SetAutoLiterary,
            Mode::AutoProgramming => Action::SetAutoProgramming,
        }
    }

    fn hotkey(self, hotkeys: &Hotkeys) -> Option<Hotkey> {
        match self {
            Action::ConvertWord => Some(hotkeys.convert_word),
            Action::ConvertSelection => hotkeys.convert_selection,
            Action::InvertCase => hotkeys.invert_case,
            Action::ToggleAutoswitch => hotkeys.toggle_autoswitch,
            Action::SetManual | Action::SetAutoLiterary | Action::SetAutoProgramming => None,
        }
    }
}

const ACTIONS: [Action; 4] =
    [Action::ConvertWord, Action::ConvertSelection, Action::InvertCase, Action::ToggleAutoswitch];

struct State {
    config: Config,
    /// Layouts of the conversion cycle: resolved when the daemon starts and when the config is reloaded.
    layouts: Vec<Layout>,
    buffer: WordBuffer,
    /// Window the buffer belongs to.
    buffer_hwnd: isize,
    /// Cached "is the foreground app ignored" answer for `ignored_hwnd`.
    ignored_hwnd: isize,
    ignored: bool,
    /// The key of a hotkey whose press was swallowed (its release and auto-repeat are swallowed too).
    swallowed_vk: Option<u32>,
    /// The automatic mode that the toggle hotkey returns to.
    last_auto: Mode,
}

thread_local! {
    static STATE: RefCell<Option<State>> = const { RefCell::new(None) };
}

fn with_state<R>(f: impl FnOnce(&mut State) -> R) -> Option<R> {
    STATE.with(|s| s.try_borrow_mut().ok().and_then(|mut s| s.as_mut().map(f)))
}

/// Applies a (re)loaded config and re-reads the keyboard layouts installed in the system.
pub fn set_config(config: Config) {
    let layouts = layout::resolve_layouts(&layout::enumerate(), &config.common);
    tracing::info!(
        "layouts: {}",
        layouts.iter().map(|l| l.id.as_str()).collect::<Vec<_>>().join(", ")
    );
    STATE.with(|s| {
        let mut slot = s.borrow_mut();
        match slot.as_mut() {
            Some(state) => {
                if state.config.common.mode != config.common.mode {
                    log_mode(config.common.mode);
                }
                if config.common.mode.is_auto() {
                    state.last_auto = config.common.mode;
                }
                state.config = config;
                state.layouts = layouts;
                state.ignored_hwnd = 0;
            }
            None => {
                log_mode(config.common.mode);
                *slot = Some(State {
                    last_auto: if config.common.mode.is_auto() { config.common.mode } else { Mode::AutoLiterary },
                    config,
                    layouts,
                    buffer: WordBuffer::default(),
                    buffer_hwnd: 0,
                    ignored_hwnd: 0,
                    ignored: false,
                    swallowed_vk: None,
                })
            }
        }
    });
}

fn log_mode(mode: Mode) {
    if mode.is_auto() {
        tracing::info!("mode = {} (the automatic switching is not implemented yet)", mode.as_str());
    } else {
        tracing::info!("mode = {}", mode.as_str());
    }
}

/// The current switching mode.
pub fn current_mode() -> Mode {
    with_state(|s| s.config.common.mode).unwrap_or_default()
}

/// Posts an action to the main (hook) thread.
pub fn post_action(action: Action) {
    unsafe {
        let _ = PostThreadMessageW(MAIN_THREAD.load(Ordering::Relaxed), WM_ACTION, WPARAM(action as usize), LPARAM(0));
    }
}

/// Posts `WM_QUIT` to the main thread.
pub fn post_quit() {
    unsafe {
        let _ = PostThreadMessageW(MAIN_THREAD.load(Ordering::Relaxed), WM_QUIT, WPARAM(0), LPARAM(0));
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

fn modifiers_match(hk: &Hotkey) -> bool {
    inject::is_down(VK_CONTROL) == hk.ctrl
        && inject::is_down(VK_MENU) == hk.alt
        && inject::is_down(VK_SHIFT) == hk.shift
        && (inject::is_down(VK_LWIN) || inject::is_down(VK_RWIN)) == hk.win
}

/// The action whose hotkey is exactly the pressed key with the held modifiers.
fn match_action(hotkeys: &Hotkeys, vk: u32) -> Option<Action> {
    ACTIONS.into_iter().find(|action| {
        action.hotkey(hotkeys).is_some_and(|hk| hk.vk == vk && modifiers_match(&hk))
    })
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

    with_state(|s| {
        let key = vk.0 as u32;

        if up && s.swallowed_vk == Some(key) {
            s.swallowed_vk = None;
            return true;
        }
        if down {
            if s.swallowed_vk == Some(key) {
                return true; // auto-repeat of a hotkey
            }
            if let Some(action) = match_action(&s.config.hotkeys, key) {
                if !foreground_ignored(s) {
                    s.swallowed_vk = Some(key);
                    if action.hotkey(&s.config.hotkeys).is_some_and(|hk| hk.alt || hk.win) {
                        inject::mask_menu_activation();
                    }
                    post_action(action);
                    return true;
                }
            }
        }

        if !down || is_modifier(vk) || foreground_ignored(s) {
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

/// Runs an action in the main thread. Returns true if the tray menu has to be refreshed.
pub fn handle_action(action: Action) -> bool {
    match action {
        Action::ConvertWord => convert_word(),
        Action::ConvertSelection => start_selection(Kind::ConvertLayout, action),
        Action::InvertCase => start_selection(Kind::InvertCase, action),
        Action::ToggleAutoswitch => return toggle_mode(),
        Action::SetManual => return set_mode(Mode::Manual),
        Action::SetAutoLiterary => return set_mode(Mode::AutoLiterary),
        Action::SetAutoProgramming => return set_mode(Mode::AutoProgramming),
    }
    false
}

/// The toggle hotkey: `manual` <-> the last automatic mode.
fn toggle_mode() -> bool {
    let Some((target, hotkey)) = with_state(|s| {
        let target = if s.config.common.mode.is_auto() { Mode::Manual } else { s.last_auto };
        (target, s.config.hotkeys.toggle_autoswitch)
    }) else {
        return false;
    };
    if let Some(hotkey) = hotkey {
        inject::release_hotkey_modifiers(&hotkey);
    }
    set_mode(target)
}

/// Selects the mode and stores it in the config file (the settings follow the file).
fn set_mode(mode: Mode) -> bool {
    let Some(config) = with_state(|s| {
        s.config.common.mode = mode;
        if mode.is_auto() {
            s.last_auto = mode;
        }
        s.config.clone()
    }) else {
        return false;
    };
    log_mode(mode);
    if let Err(e) = config.save() {
        tracing::warn!("cannot save the config: {e}");
    }
    true
}

fn start_selection(kind: Kind, action: Action) {
    let hwnd = inject::foreground();
    let request = with_state(|s| {
        if s.layouts.len() < 2 && kind == Kind::ConvertLayout {
            return None;
        }
        Some(selection::Request {
            kind,
            hwnd: hwnd.0 as isize,
            hotkey: action.hotkey(&s.config.hotkeys)?,
            layouts: s.layouts.clone(),
            current_id: layout::describe(inject::window_hkl(hwnd)).id,
        })
    })
    .flatten();
    if let Some(request) = request {
        selection::start(request);
    }
}

/// Converts the last typed word (the layout cycle continues on every press).
fn convert_word() {
    let hwnd = inject::foreground();
    let plan = with_state(|s| {
        if hwnd.0 as isize != s.buffer_hwnd || s.buffer.is_empty() {
            return None;
        }
        let origin = layout::describe(inject::window_hkl(hwnd)).id;
        let layouts = &s.layouts;
        let replacement = s.buffer.next_replacement(
            || {
                let seq = layout::cycle_sequence(layouts, Some(&origin));
                tracing::debug!(
                    "new cycle: current layout {origin}, order {}",
                    seq.iter().map(|l| l.id.as_str()).collect::<Vec<_>>().join(" > ")
                );
                seq
            },
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
