//! Low-level keyboard / mouse hooks, the typed-word tracking and the hotkey actions.

use std::cell::RefCell;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{Duration, Instant};

use windows::Win32::Foundation::{HINSTANCE, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetKeyState, VIRTUAL_KEY, VK_BACK, VK_CAPITAL, VK_CONTROL, VK_DELETE, VK_DOWN, VK_ESCAPE, VK_LCONTROL,
    VK_LEFT, VK_LMENU, VK_LSHIFT, VK_LWIN, VK_MENU, VK_NUMLOCK, VK_RCONTROL, VK_RETURN, VK_RIGHT, VK_RMENU,
    VK_RSHIFT, VK_RWIN, VK_SCROLL, VK_SHIFT, VK_SPACE, VK_TAB, VK_UP,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, PostThreadMessageW, SetWindowsHookExW, UnhookWindowsHookEx, HHOOK, KBDLLHOOKSTRUCT,
    LLKHF_EXTENDED, WH_KEYBOARD_LL, WH_MOUSE_LL, WM_APP, WM_KEYDOWN, WM_KEYUP, WM_LBUTTONDOWN,
    WM_MBUTTONDOWN, WM_QUIT, WM_RBUTTONDOWN, WM_SYSKEYDOWN, WM_SYSKEYUP,
};

use lite_switcher_core::config::{Config, Hotkeys, Mode};
use lite_switcher_core::hotkey::Hotkey;
use lite_switcher_core::layout::{self, key_to_char, Layout};
use lite_switcher_core::word::{taint, KeyStroke, WordBuffer};
use lite_switcher_lang::literary::{self, Context};
use lite_switcher_lang::programming;
use lite_switcher_lang::render::{KeyClass, KeyRenderer};

use crate::engine::{self, Engine};
use crate::inject::{self, MARKER};
use crate::selection::{self, Kind};
use crate::auto;

/// Posted to the main thread when a hotkey is pressed; `wParam` is the `Action`.
pub const WM_ACTION: u32 = WM_APP + 1;
/// Posted to the main thread when the language packs have been (re)loaded.
pub const WM_PACKS_LOADED: u32 = WM_APP + 2;
/// Posted to the main thread when a word has to be replaced automatically (see `auto`).
pub const WM_AUTO_REPLACE: u32 = WM_APP + 3;

/// How long the language of the previous word helps to decide between similar languages.
const PREVIOUS_WORD_TTL: Duration = Duration::from_secs(30);

/// The language (layout) of the previous word.
struct Prev {
    layout_id: String,
    at: Instant,
    hwnd: isize,
}

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
    /// The language of the previous word (for ru/uk ties).
    prev: Option<Prev>,
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
                    prev: None,
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
    tracing::info!("mode = {}", mode.as_str());
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

/// Posts a message without parameters to the main thread (callable from any thread).
pub fn post_message(message: u32) {
    unsafe {
        let _ = PostThreadMessageW(MAIN_THREAD.load(Ordering::Relaxed), message, WPARAM(0), LPARAM(0));
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
            // a word is being replaced: the keys of the user wait until it is done
            if auto::hold_if_replacing(kb, wparam.0 as u32) {
                return LRESULT(1);
            }
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
        track_key(s, vk, kb)
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

/// Keeps the word as it is after the key that ended it.
fn end_word(s: &mut State, key: Option<KeyStroke>) {
    match key {
        Some(k) => s.buffer.trailing_key(k, s.config.common.keep_trailing_space),
        None => s.buffer.clear(),
    }
}

/// The digit keys of the main row: part of a word ("abc123"), not its end.
fn is_digit_key(key: &KeyStroke) -> bool {
    !key.shift && (0x02..=0x0B).contains(&key.scancode)
}

/// Shift + the key of the minus: `_`, part of an identifier.
fn is_underscore_key(key: &KeyStroke) -> bool {
    key.shift && key.scancode == 0x0C
}

/// What the detector of the current mode says about the word.
enum Decision {
    Keep(String),
    Replace {
        /// The word in the right layout (with the punctuation around it).
        text: String,
        /// The layout to switch to; `None` = keep the layout of the window.
        layout: Option<usize>,
        info: String,
    },
}

/// Notices that the layout of the window changed while the word was typed.
fn note_layout(s: &mut State, hkl: isize) {
    let Some(start) = s.buffer.start_hkl() else { return };
    if start == hkl || s.buffer.is_empty() {
        return;
    }
    if s.config.troubleshooting.no_switch.change_layout {
        s.buffer.taint(taint::CHANGE_LAYOUT);
        s.buffer.set_start_hkl(hkl);
    } else {
        s.buffer.restart_in(hkl);
    }
}

/// Tracks the typed word. Returns true if the key must be swallowed (it ended a word that is
/// being replaced automatically).
fn track_key(s: &mut State, vk: VIRTUAL_KEY, kb: &KBDLLHOOKSTRUCT) -> bool {
    let ctrl_alt_win = inject::is_down(VK_CONTROL)
        || inject::is_down(VK_MENU)
        || inject::is_down(VK_LWIN)
        || inject::is_down(VK_RWIN);
    if ctrl_alt_win {
        s.buffer.clear();
        return false;
    }
    let ns = s.config.troubleshooting.no_switch;
    match vk {
        VK_BACK => {
            s.buffer.backspace();
            if ns.backspace {
                s.buffer.taint(taint::BACKSPACE);
            }
            return false;
        }
        // the caret moves: the word is over, and the text typed after the move is only a fragment
        VK_LEFT | VK_RIGHT | VK_UP | VK_DOWN => {
            let (checked, bit) = match vk {
                VK_LEFT => (ns.left, taint::LEFT),
                VK_RIGHT => (ns.right, taint::RIGHT),
                VK_UP => (ns.up, taint::UP),
                _ => (ns.down, taint::DOWN),
            };
            s.buffer.clear();
            if checked {
                s.buffer.taint(bit);
            }
            return false;
        }
        // Delete removes what is after the caret: the typed word stays intact
        VK_DELETE => {
            if ns.delete {
                s.buffer.taint(taint::DELETE);
            }
            return false;
        }
        VK_TAB | VK_ESCAPE => {
            s.buffer.clear();
            return false;
        }
        // numpad keys and the other extended keys (Home, End, Insert...) are not part of a word
        _ if (0x60..=0x6F).contains(&vk.0) || kb.flags.0 & LLKHF_EXTENDED.0 != 0 => {
            s.buffer.clear();
            return false;
        }
        _ => {}
    }

    let hwnd = inject::foreground();
    let hkl = inject::window_hkl(hwnd);
    let shift = inject::is_down(VK_SHIFT);
    let caps = unsafe { GetKeyState(VK_CAPITAL.0 as i32) } & 1 != 0;
    let scancode = kb.scanCode as u16;
    let key = KeyStroke { scancode, shift, caps };
    let engine = engine::current();
    let mode = s.config.common.mode;
    let auto = match mode {
        Mode::AutoLiterary => engine.literary_ready(),
        Mode::AutoProgramming => engine.programming_ready(),
        Mode::Manual => false,
    };

    if vk == VK_RETURN {
        if auto {
            return on_trigger(s, &engine, None, vk, hkl, hwnd.0 as isize);
        }
        s.buffer.clear();
        return false;
    }
    if !auto {
        // manual mode: every printable key is part of the word, a space is kept after it
        if vk == VK_SPACE {
            s.buffer.space(s.config.common.keep_trailing_space);
        } else if key_to_char(hkl, scancode, shift, caps).is_some() {
            s.buffer.push_in(key, hkl);
        } else {
            s.buffer.clear();
        }
        return false;
    }
    match engine.table.class(&key) {
        KeyClass::None => {
            s.buffer.clear();
            false
        }
        // in a program the identifiers have digits and `_`, and a token may start with `#` or `&`
        KeyClass::Separator
            if mode == Mode::AutoProgramming
                && (is_digit_key(&key)
                    || is_underscore_key(&key)
                    || (s.buffer.is_empty() && engine.programming.as_ref().is_some_and(|p| p.is_sigil_key(&key)))) =>
        {
            note_layout(s, hkl);
            s.buffer.push_in(key, hkl);
            false
        }
        KeyClass::Separator if !is_digit_key(&key) => on_trigger(s, &engine, Some(key), vk, hkl, hwnd.0 as isize),
        // letters, keys that are letters in some layouts, digits
        _ => {
            note_layout(s, hkl);
            s.buffer.push_in(key, hkl);
            false
        }
    }
}

/// The key `key` (`None` = Enter) ended the word: decide whether to replace it. Returns true if
/// the key was swallowed because the word is being replaced.
fn on_trigger(s: &mut State, engine: &Engine, key: Option<KeyStroke>, vk: VIRTUAL_KEY, hkl: isize, hwnd: isize) -> bool {
    // nothing typed, or a second separator in a row: the word was already judged
    if s.buffer.is_empty() || !s.buffer.trailing().is_empty() {
        end_word(s, key);
        return false;
    }
    note_layout(s, hkl);
    if s.buffer.is_tainted() {
        tracing::debug!("auto: skipped, an unwanted key was pressed (taint {:#b})", s.buffer.taint_bits());
        end_word(s, key);
        return false;
    }
    let current_id = layout::describe(hkl).id;
    let Some(current) = engine.table.layout_index(&current_id) else {
        end_word(s, key);
        return false;
    };
    let started = Instant::now();
    let decision = if s.config.common.mode == Mode::AutoProgramming {
        let Some(detector) = engine.programming.as_ref() else {
            end_word(s, key);
            return false;
        };
        match detector.decide(&engine.table, &Context { keys: s.buffer.keys(), current, previous: None }) {
            programming::Verdict::Keep(reason) => Decision::Keep(format!("{reason:?}")),
            programming::Verdict::Replace(r) => {
                Decision::Replace { text: r.text, layout: r.layout, info: format!("{:?}, hybrid {}", r.kind, r.hybrid) }
            }
        }
    } else {
        let previous = s
            .prev
            .as_ref()
            .filter(|p| p.hwnd == hwnd && p.at.elapsed() < PREVIOUS_WORD_TTL)
            .and_then(|p| engine.table.layout_index(&p.layout_id));
        match engine.literary.decide(&engine.table, &Context { keys: s.buffer.keys(), current, previous }) {
            literary::Verdict::Keep(reason) => {
                if matches!(reason, literary::KeepReason::Dictionary | literary::KeepReason::Exception) {
                    s.prev = Some(Prev { layout_id: current_id.clone(), at: Instant::now(), hwnd });
                }
                Decision::Keep(format!("{reason:?}"))
            }
            literary::Verdict::Replace(r) => {
                Decision::Replace { text: r.text, layout: Some(r.layout), info: format!("margin {}, dictionary {}", r.margin, r.dictionary) }
            }
        }
    };
    let elapsed = started.elapsed();
    let (text, layout, info) = match decision {
        Decision::Keep(reason) => {
            tracing::debug!("auto: keep ({reason}) in {elapsed:?}");
            end_word(s, key);
            return false;
        }
        Decision::Replace { text, layout, info } => (text, layout, info),
    };

    // the key that ended the word is typed again after the word, as it looks in the layout of the word
    let shown_layout = layout.unwrap_or(current);
    let target = engine.layouts[shown_layout].clone();
    let separator = match key {
        Some(k) => match engine.table.render(&k, shown_layout) {
            Some(c) => c.to_string(),
            None => {
                end_word(s, key);
                return false;
            }
        },
        None => String::new(),
    };
    let seq = layout::cycle_sequence(&engine.layouts, Some(&current_id));
    let Some(pos) = seq.iter().position(|l| l.id == target.id) else {
        end_word(s, key);
        return false;
    };
    tracing::debug!(
        "auto: replace {} chars, layout {} -> {} ({}), {info}, decided in {elapsed:?}",
        s.buffer.shown_len(),
        current_id,
        target.id,
        if layout.is_some() { "switched" } else { "kept" }
    );
    let plan = auto::Plan {
        hwnd,
        backspaces: s.buffer.shown_len(),
        text: format!("{text}{separator}"),
        enter: key.is_none(),
        target_hkl: layout.map(|_| target.hkl),
        key_delay_ms: s.config.troubleshooting.key_delay_ms,
        sound: s.config.sounds.enabled.then(|| s.config.sounds.on_convert.clone()),
    };
    // from now on the buffer is the replaced word; the hotkey continues the cycle from this layout
    match key {
        Some(k) => s.buffer.after_auto_replacement(text.chars().count(), vec![k], seq, pos),
        None => s.buffer.clear(),
    }
    s.buffer.set_start_hkl(target.hkl);
    s.prev = Some(Prev { layout_id: target.id.clone(), at: Instant::now(), hwnd });
    s.swallowed_vk = Some(u32::from(vk.0));
    auto::start(plan);
    true
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
        // the window gets the new layout on purpose: that is not a "change layout" key
        s.buffer.set_start_hkl(replacement.target.hkl);
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
