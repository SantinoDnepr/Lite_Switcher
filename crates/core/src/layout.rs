//! Keyboard layouts installed in the system and the scancode -> character mapping for each of them.

use std::ffi::c_void;

use windows::core::PCWSTR;
use windows::Win32::Globalization::{GetLocaleInfoEx, LCIDToLocaleName, LOCALE_SLOCALIZEDDISPLAYNAME};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetKeyboardLayoutList, MapVirtualKeyExW, ToUnicodeEx, HKL, MAPVK_VSC_TO_VK_EX, VK_CAPITAL, VK_SHIFT,
};

use crate::config::Common;

/// An installed keyboard layout.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Layout {
    /// Raw HKL value.
    pub hkl: isize,
    /// Locale name of the layout language, e.g. `uk-UA`. Used in the config file.
    pub id: String,
    /// Human readable language name.
    pub name: String,
}

pub fn hkl_from_raw(raw: isize) -> HKL {
    HKL(raw as *mut c_void)
}

fn wide_to_string(buf: &[u16], len: i32) -> String {
    let len = (len.max(1) as usize - 1).min(buf.len());
    String::from_utf16_lossy(&buf[..len])
}

/// Describes a layout by its HKL (the low word of an HKL is the language id).
pub fn describe(hkl: isize) -> Layout {
    let lcid = (hkl as usize & 0xFFFF) as u32;
    let mut name = [0u16; 85];
    let n = unsafe { LCIDToLocaleName(lcid, Some(&mut name), 0) };
    let id = if n > 0 { wide_to_string(&name, n) } else { format!("{lcid:04x}") };

    let mut display = [0u16; 128];
    let n = unsafe {
        GetLocaleInfoEx(PCWSTR(name.as_ptr()), LOCALE_SLOCALIZEDDISPLAYNAME, Some(&mut display))
    };
    let name = if n > 0 { wide_to_string(&display, n) } else { id.clone() };
    Layout { hkl, id, name }
}

/// All layouts installed in the system, in the system order.
pub fn enumerate() -> Vec<Layout> {
    unsafe {
        let count = GetKeyboardLayoutList(None);
        if count <= 0 {
            return Vec::new();
        }
        let mut buf = vec![HKL::default(); count as usize];
        let count = GetKeyboardLayoutList(Some(&mut buf));
        buf.iter().take(count.max(0) as usize).map(|h| describe(h.0 as isize)).collect()
    }
}

/// Applies the `[common]` settings to the system layouts: dedups by id, then either takes the
/// explicitly listed layouts (in the listed order) or all of them minus the excluded ones.
pub fn resolve_layouts(system: &[Layout], common: &Common) -> Vec<Layout> {
    let mut unique: Vec<&Layout> = Vec::new();
    for l in system {
        if !unique.iter().any(|u| u.id.eq_ignore_ascii_case(&l.id)) {
            unique.push(l);
        }
    }
    if common.layouts.is_empty() {
        unique
            .into_iter()
            .filter(|l| !common.exclude_layouts.iter().any(|e| e.eq_ignore_ascii_case(&l.id)))
            .cloned()
            .collect()
    } else {
        let mut out: Vec<Layout> = Vec::new();
        for id in &common.layouts {
            if let Some(l) = unique.iter().find(|l| l.id.eq_ignore_ascii_case(id)) {
                if !out.iter().any(|o| o.id == l.id) {
                    out.push((*l).clone());
                }
            }
        }
        out
    }
}

/// Conversion cycle for a word typed in `origin`: all other layouts in order, then the origin itself.
pub fn cycle_sequence(layouts: &[Layout], origin_id: Option<&str>) -> Vec<Layout> {
    let is_origin = |l: &Layout| origin_id.is_some_and(|o| o.eq_ignore_ascii_case(&l.id));
    let mut seq: Vec<Layout> = layouts.iter().filter(|l| !is_origin(l)).cloned().collect();
    seq.extend(layouts.iter().filter(|l| is_origin(l)).cloned());
    seq
}

/// The character produced by a physical key (scancode) in the given layout, if it is a printable one.
pub fn key_to_char(hkl: isize, scancode: u16, shift: bool, caps: bool) -> Option<char> {
    let hkl = hkl_from_raw(hkl);
    unsafe {
        let vk = MapVirtualKeyExW(scancode as u32, MAPVK_VSC_TO_VK_EX, Some(hkl));
        if vk == 0 {
            return None;
        }
        let mut state = [0u8; 256];
        if shift {
            state[VK_SHIFT.0 as usize] = 0x80;
        }
        if caps {
            state[VK_CAPITAL.0 as usize] = 0x01;
        }
        let mut buf = [0u16; 8];
        // flag 0x4: do not change the keyboard state (dead keys stay untouched)
        let n = ToUnicodeEx(vk, scancode as u32, &state, &mut buf, 0x4, Some(hkl));
        if n < 1 {
            return None;
        }
        char::decode_utf16(buf[..n as usize].iter().copied())
            .next()
            .and_then(|r| r.ok())
            .filter(|c| !c.is_control())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::word::KeyStroke;
    use windows::core::w;
    use windows::Win32::UI::Input::KeyboardAndMouse::{LoadKeyboardLayoutW, KLF_NOTELLSHELL};

    fn load(klid: PCWSTR) -> Option<isize> {
        unsafe { LoadKeyboardLayoutW(klid, KLF_NOTELLSHELL).ok().map(|h| h.0 as isize) }
    }

    fn render(keys: &[KeyStroke], hkl: isize) -> String {
        keys.iter().filter_map(|k| key_to_char(hkl, k.scancode, k.shift, k.caps)).collect()
    }

    fn keys(scancodes: &[u16]) -> Vec<KeyStroke> {
        scancodes.iter().map(|&scancode| KeyStroke { scancode, shift: false, caps: false }).collect()
    }

    #[test]
    fn enumerates_system_layouts() {
        let list = enumerate();
        assert!(!list.is_empty());
        assert!(list.iter().all(|l| !l.id.is_empty()));
    }

    #[test]
    fn word_converts_between_layouts() {
        let (Some(en), Some(ru), Some(uk)) =
            (load(w!("00000409")), load(w!("00000419")), load(w!("00000422")))
        else {
            eprintln!("EN/RU/UK layouts are unavailable, skipping");
            return;
        };
        // scancodes: w o r d
        let word = keys(&[0x11, 0x18, 0x13, 0x20]);
        assert_eq!(render(&word, en), "word");
        assert_eq!(render(&word, ru), "цщкв");
        assert_eq!(render(&word, uk), "цщкв");
        // scancodes: s ] '  (ru: ы ъ э, uk: і ї є)
        let diff = keys(&[0x1F, 0x1B, 0x28]);
        assert_eq!(render(&diff, ru), "ыъэ");
        assert_eq!(render(&diff, uk), "іїє");
        // shift gives upper case
        let shifted = [KeyStroke { scancode: 0x11, shift: true, caps: false }];
        assert_eq!(render(&shifted, en), "W");
        assert_eq!(render(&shifted, ru), "Ц");
        // caps lock gives upper case too
        let caps = [KeyStroke { scancode: 0x11, shift: false, caps: true }];
        assert_eq!(render(&caps, ru), "Ц");
    }

    fn l(id: &str, hkl: isize) -> Layout {
        Layout { hkl, id: id.into(), name: id.into() }
    }

    #[test]
    fn resolve_auto_exclude_and_explicit() {
        let sys = vec![l("en-US", 1), l("ru-RU", 2), l("uk-UA", 3), l("en-US", 4)];
        let mut common = crate::config::Config::default().common;
        let ids = |v: &[Layout]| v.iter().map(|l| l.id.clone()).collect::<Vec<_>>();
        assert_eq!(ids(&resolve_layouts(&sys, &common)), ["en-US", "ru-RU", "uk-UA"]);

        common.exclude_layouts = vec!["RU-ru".into()];
        assert_eq!(ids(&resolve_layouts(&sys, &common)), ["en-US", "uk-UA"]);

        common.layouts = vec!["uk-UA".into(), "xx-XX".into(), "en-US".into()];
        assert_eq!(ids(&resolve_layouts(&sys, &common)), ["uk-UA", "en-US"]);
    }

    #[test]
    fn cycle_puts_origin_last() {
        let all = vec![l("en-US", 1), l("ru-RU", 2), l("uk-UA", 3)];
        let ids = |v: &[Layout]| v.iter().map(|l| l.id.clone()).collect::<Vec<_>>();
        assert_eq!(ids(&cycle_sequence(&all, Some("ru-RU"))), ["en-US", "uk-UA", "ru-RU"]);
        assert_eq!(ids(&cycle_sequence(&all, Some("en-US"))), ["ru-RU", "uk-UA", "en-US"]);
        assert_eq!(ids(&cycle_sequence(&all, None)), ["en-US", "ru-RU", "uk-UA"]);
    }
}
