//! Actions on the selected text: layout conversion and case inversion.
//!
//! The selection is read with Ctrl+C and replaced with Ctrl+V; the clipboard of the user is put back.
//! The work runs in its own thread because it has to wait for the target application, while the
//! keyboard hook (in the main thread) must keep answering.

use std::ffi::c_void;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::thread::{self, sleep};
use std::time::Duration;

use windows::Win32::Foundation::HWND;
use windows::Win32::UI::Input::KeyboardAndMouse::{VIRTUAL_KEY, VK_CONTROL};

use lite_switcher_core::hotkey::Hotkey;
use lite_switcher_core::layout::{self, Layout};
use lite_switcher_core::text;

use crate::{clip, inject};

const VK_C: VIRTUAL_KEY = VIRTUAL_KEY(0x43);
const VK_V: VIRTUAL_KEY = VIRTUAL_KEY(0x56);
/// How long the application gets to copy the selection.
const COPY_WAIT_MS: u32 = 600;
/// How long the application gets to read the clipboard after Ctrl+V, before the clipboard is restored.
const PASTE_WAIT_MS: u64 = 250;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// Retype the selection in the next layout of the list.
    ConvertLayout,
    InvertCase,
}

pub struct Request {
    pub kind: Kind,
    /// Raw handle of the foreground window.
    pub hwnd: isize,
    /// The hotkey that was pressed (its modifiers may still be held).
    pub hotkey: Hotkey,
    pub layouts: Vec<Layout>,
    /// Id of the layout that is active now.
    pub current_id: String,
}

static BUSY: AtomicBool = AtomicBool::new(false);
/// The last converted text and the id of the layout it was converted to: pressing the hotkey again
/// on that text continues the cycle from this layout.
static LAST: Mutex<Option<(String, String)>> = Mutex::new(None);

/// Starts the action in a background thread; ignored while the previous one is still running.
pub fn start(request: Request) {
    if BUSY.swap(true, Ordering::SeqCst) {
        return;
    }
    thread::spawn(move || {
        let result = run(&request);
        BUSY.store(false, Ordering::SeqCst);
        match result {
            Ok(message) => tracing::debug!("selection {:?}: {message}", request.kind),
            Err(e) => tracing::warn!("selection {:?} failed: {e}", request.kind),
        }
    });
}

fn run(req: &Request) -> Result<&'static str, String> {
    inject::release_hotkey_modifiers(&req.hotkey);
    sleep(Duration::from_millis(30));

    let snapshot = clip::snapshot().ok_or("the clipboard is busy")?;
    let sequence = clip::sequence();
    inject::send_chord(VK_CONTROL, VK_C);
    let copied = (0..COPY_WAIT_MS / 10).any(|_| {
        sleep(Duration::from_millis(10));
        clip::sequence() != sequence
    });
    tracing::debug!("copy: clipboard sequence {sequence} -> {}", clip::sequence());
    if !copied {
        return Ok("nothing is selected"); // the clipboard was not touched
    }

    // Some applications fill the clipboard in several steps.
    let text = (0..10)
        .find_map(|_| {
            sleep(Duration::from_millis(15));
            clip::get_text().filter(|t| !t.is_empty())
        })
        .ok_or_else(|| {
            clip::restore(&snapshot);
            "the selection is not text"
        })?;

    tracing::debug!("copy: {} chars", text.chars().count());
    let (new_text, target) = match req.kind {
        Kind::InvertCase => (text::invert_case(&text), None),
        Kind::ConvertLayout => {
            let Some((new_text, target)) = convert_layout(req, &text) else {
                clip::restore(&snapshot);
                return Ok("nothing to convert");
            };
            (new_text, Some(target))
        }
    };
    if new_text == text {
        // e.g. "цщкв" in ru and uk: nothing to paste, but the cycle has to move on
        clip::restore(&snapshot);
        if let Some(target) = target.filter(|_| text.chars().any(char::is_alphabetic)) {
            finish_convert(req, &text, &target);
        }
        return Ok("text is unchanged");
    }

    if !clip::set_text(&new_text) {
        clip::restore(&snapshot);
        return Err("cannot write the clipboard".into());
    }
    inject::send_chord(VK_CONTROL, VK_V);
    sleep(Duration::from_millis(PASTE_WAIT_MS));
    // keep the result selected, so that the hotkey can be pressed again
    inject::select_left(text::caret_steps(&new_text));
    sleep(Duration::from_millis(30));
    clip::restore(&snapshot);

    if let Some(target) = target {
        finish_convert(req, &new_text, &target);
    }
    tracing::debug!("selection: {} chars replaced", new_text.chars().count());
    Ok("converted")
}

/// Switches the layout of the window and remembers where the text is now.
fn finish_convert(req: &Request, text: &str, target: &Layout) {
    inject::switch_layout(HWND(req.hwnd as *mut c_void), target.hkl);
    if let Ok(mut last) = LAST.lock() {
        *last = Some((text.to_string(), target.id.clone()));
    }
}

/// The converted text and the layout it was converted to.
fn convert_layout(req: &Request, text: &str) -> Option<(String, Layout)> {
    let layouts = &req.layouts;
    let last = LAST.lock().ok().and_then(|l| l.clone());
    let source = last
        .filter(|(t, _)| t == text)
        .and_then(|(_, id)| layouts.iter().position(|l| l.id == id))
        .or_else(|| text::pick_source(text, layouts, Some(&req.current_id)))?;
    let source = &layouts[source];
    let target = layout::cycle_sequence(layouts, Some(&source.id)).into_iter().next()?;
    if target.id == source.id {
        return None;
    }
    Some((text::convert_text(text, source, &target), target))
}
