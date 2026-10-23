//! Transformations of an arbitrary text (the selected text): layout conversion and case inversion.

use std::collections::HashMap;

use crate::layout::{key_to_char, Layout};

/// Scancodes of the typewriter block (digits, letters, punctuation) and the extra key of ISO keyboards.
const SCANCODES: [u16; 49] = [
    0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0A, 0x0B, 0x0C, 0x0D, 0x10, 0x11, 0x12, 0x13, 0x14,
    0x15, 0x16, 0x17, 0x18, 0x19, 0x1A, 0x1B, 0x1E, 0x1F, 0x20, 0x21, 0x22, 0x23, 0x24, 0x25, 0x26, 0x27,
    0x28, 0x29, 0x2B, 0x2C, 0x2D, 0x2E, 0x2F, 0x30, 0x31, 0x32, 0x33, 0x34, 0x35, 0x56, 0x39,
];

/// Which physical key (scancode, shift) types each character in the layout.
pub type CharTable = HashMap<char, (u16, bool)>;

pub fn char_table(hkl: isize) -> CharTable {
    let mut table = CharTable::new();
    for shift in [false, true] {
        for &scancode in &SCANCODES {
            if let Some(c) = key_to_char(hkl, scancode, shift, false) {
                table.entry(c).or_insert((scancode, shift));
            }
        }
    }
    table
}

/// Picks the layout the text was typed in: the one that can type most of its letters.
/// On a tie the `prefer` layout (usually the current one) wins, then the list order.
pub fn pick_source(text: &str, layouts: &[Layout], prefer: Option<&str>) -> Option<usize> {
    let mut best: Option<(usize, usize, bool)> = None; // (index, score, preferred)
    for (i, layout) in layouts.iter().enumerate() {
        let table = char_table(layout.hkl);
        let score = text.chars().filter(|c| c.is_alphabetic() && table.contains_key(c)).count();
        let preferred = prefer.is_some_and(|p| p.eq_ignore_ascii_case(&layout.id));
        let better = match best {
            None => true,
            Some((_, s, p)) => score > s || (score == s && preferred && !p),
        };
        if better {
            best = Some((i, score, preferred));
        }
    }
    best.map(|(i, _, _)| i)
}

/// Retypes `text` as if the same keys were pressed in the `target` layout instead of the `source` one.
/// Characters that `source` cannot type (and line breaks) stay unchanged.
pub fn convert_text(text: &str, source: &Layout, target: &Layout) -> String {
    let table = char_table(source.hkl);
    text.chars()
        .map(|c| match table.get(&c) {
            Some(&(scancode, shift)) => key_to_char(target.hkl, scancode, shift, false).unwrap_or(c),
            None => c,
        })
        .collect()
}

/// Upper case becomes lower case and vice versa.
pub fn invert_case(text: &str) -> String {
    text.chars()
        .flat_map(|c| -> Vec<char> {
            if c.is_lowercase() {
                c.to_uppercase().collect()
            } else if c.is_uppercase() {
                c.to_lowercase().collect()
            } else {
                vec![c]
            }
        })
        .collect()
}

/// How many times the caret has to move left to cross the whole text (a line break counts once).
pub fn caret_steps(text: &str) -> usize {
    text.chars().count() - text.matches("\r\n").count()
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows::core::{w, PCWSTR};
    use windows::Win32::UI::Input::KeyboardAndMouse::{LoadKeyboardLayoutW, KLF_NOTELLSHELL};

    fn load(id: &str, klid: PCWSTR) -> Option<Layout> {
        let hkl = unsafe { LoadKeyboardLayoutW(klid, KLF_NOTELLSHELL).ok()?.0 as isize };
        Some(Layout { hkl, id: id.into(), name: id.into() })
    }

    fn en_ru_uk() -> Option<Vec<Layout>> {
        Some(vec![
            load("en-US", w!("00000409"))?,
            load("ru-RU", w!("00000419"))?,
            load("uk-UA", w!("00000422"))?,
        ])
    }

    #[test]
    fn inverts_case() {
        assert_eq!(invert_case("Hello, Мир 123"), "hELLO, мИР 123");
        assert_eq!(invert_case("ЦЩкв"), "цщКВ");
    }

    #[test]
    fn caret_steps_count_line_breaks_once() {
        assert_eq!(caret_steps("abc"), 3);
        assert_eq!(caret_steps("a\r\nb"), 3);
        assert_eq!(caret_steps("a\nb"), 3);
    }

    #[test]
    fn converts_text_between_layouts() {
        let Some(l) = en_ru_uk() else { return };
        assert_eq!(convert_text("Hello, world 1!", &l[0], &l[1]), "Руддщб цщкдв 1!");
        assert_eq!(convert_text("Руддщб цщкдв 1!", &l[1], &l[0]), "Hello, world 1!");
        assert_eq!(convert_text("sdf", &l[0], &l[2]), "іва");
        assert_eq!(convert_text("ыва", &l[1], &l[2]), "іва");
        assert_eq!(convert_text("line1\r\nline2", &l[0], &l[1]), "дшту1\r\nдшту2");
    }

    #[test]
    fn picks_source_layout() {
        let Some(l) = en_ru_uk() else { return };
        assert_eq!(pick_source("word", &l, None), Some(0));
        assert_eq!(pick_source("ыва", &l, None), Some(1));
        assert_eq!(pick_source("іва", &l, None), Some(2));
        // ru and uk can both type it: the preferred (current) one wins, otherwise the list order
        assert_eq!(pick_source("цщкв", &l, Some("uk-UA")), Some(2));
        assert_eq!(pick_source("цщкв", &l, Some("ru-RU")), Some(1));
        assert_eq!(pick_source("цщкв", &l, None), Some(1));
    }
}
