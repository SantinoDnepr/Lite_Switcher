//! The automatic replacement of a word and the "input gate".
//!
//! The keyboard hook decides (in microseconds) that the word has to be replaced; it swallows the key
//! that ended the word and posts a plan to the main thread. Until the replacement is finished the
//! real keys that the user types are held back in a queue (the gate) and replayed afterwards, so
//! that a letter of the next word cannot land in the middle of the word being replaced.

use std::ffi::c_void;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::thread::sleep;
use std::time::{Duration, Instant};

use windows::Win32::Foundation::HWND;
use windows::Win32::UI::WindowsAndMessaging::{KBDLLHOOKSTRUCT, LLKHF_EXTENDED, WM_KEYUP, WM_SYSKEYUP};

use crate::hook::{post_message, WM_AUTO_REPLACE};
use crate::inject;

/// The longest the gate stays closed: if the replacement takes longer, the held keys are let through.
pub const GATE_TIMEOUT: Duration = Duration::from_millis(200);
/// How long to wait for the window to take the new layout before the held keys are replayed.
const LAYOUT_WAIT: Duration = Duration::from_millis(40);

/// What to do in the window to replace the word.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Plan {
    /// Raw handle of the foreground window.
    pub hwnd: isize,
    /// Characters of the word to erase.
    pub backspaces: usize,
    /// The word in the right layout, with the punctuation or space that ended it.
    pub text: String,
    /// The word was ended with Enter: press it after the text.
    pub enter: bool,
    /// Raw HKL of the layout to switch the window to; `None` = keep the layout of the window.
    pub target_hkl: Option<isize>,
    pub key_delay_ms: u32,
    /// Sound to play after the replacement (empty path = the system sound).
    pub sound: Option<String>,
}

/// A key the user pressed while the gate was closed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HeldKey {
    pub vk: u16,
    pub scan: u16,
    pub up: bool,
    pub extended: bool,
}

static PLAN: Mutex<Option<Plan>> = Mutex::new(None);
static GATE_OPEN_SINCE: Mutex<Option<Instant>> = Mutex::new(None);
static HELD: Mutex<Vec<HeldKey>> = Mutex::new(Vec::new());
static REPLACING: AtomicBool = AtomicBool::new(false);

/// Closes the gate and hands the plan to the main thread.
pub fn start(plan: Plan) {
    if let Ok(mut slot) = PLAN.lock() {
        *slot = Some(plan);
    }
    if let Ok(mut since) = GATE_OPEN_SINCE.lock() {
        *since = Some(Instant::now());
    }
    REPLACING.store(true, Ordering::SeqCst);
    post_message(WM_AUTO_REPLACE);
}

/// Is a replacement in progress (the gate is closed)?
pub fn replacing() -> bool {
    REPLACING.load(Ordering::SeqCst)
}

/// Called by the keyboard hook for every real key event. While the gate is closed the key is held
/// back and `true` is returned (swallow it). After the timeout the gate opens by itself.
pub fn hold_if_replacing(kb: &KBDLLHOOKSTRUCT, message: u32) -> bool {
    if !replacing() {
        return false;
    }
    let key = HeldKey {
        vk: kb.vkCode as u16,
        scan: kb.scanCode as u16,
        up: matches!(message, WM_KEYUP | WM_SYSKEYUP),
        extended: kb.flags.0 & LLKHF_EXTENDED.0 != 0,
    };
    if let Ok(mut held) = HELD.lock() {
        held.push(key);
    }
    let expired = GATE_OPEN_SINCE.lock().ok().and_then(|s| *s).is_some_and(|t| t.elapsed() > GATE_TIMEOUT);
    if expired {
        tracing::warn!("the input gate timed out");
        open_gate();
    }
    true
}

/// Opens the gate and replays the held keys in their order.
pub fn open_gate() {
    REPLACING.store(false, Ordering::SeqCst);
    if let Ok(mut since) = GATE_OPEN_SINCE.lock() {
        *since = None;
    }
    let held: Vec<HeldKey> = HELD.lock().map(|mut h| std::mem::take(&mut *h)).unwrap_or_default();
    if !held.is_empty() {
        inject::replay_keys(&held);
    }
}

/// Runs the plan (main thread, on `WM_AUTO_REPLACE`) and opens the gate.
pub fn run() {
    let plan = PLAN.lock().ok().and_then(|mut slot| slot.take());
    if let Some(plan) = plan {
        let hwnd = HWND(plan.hwnd as *mut c_void);
        inject::send_backspaces(plan.backspaces, plan.key_delay_ms);
        if let Some(target) = plan.target_hkl {
            inject::switch_layout(hwnd, target);
        }
        inject::send_text(&plan.text, plan.key_delay_ms);
        if plan.enter {
            inject::send_enter();
        }
        // the window takes the new layout asynchronously: the keys typed next must see it
        if let Some(target) = plan.target_hkl {
            let started = Instant::now();
            while inject::window_hkl(hwnd) != target && started.elapsed() < LAYOUT_WAIT {
                sleep(Duration::from_millis(2));
            }
        }
        if let Some(wav) = &plan.sound {
            inject::play_sound(wav);
        }
    }
    open_gate();
}
