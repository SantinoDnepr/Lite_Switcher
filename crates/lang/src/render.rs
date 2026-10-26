//! The key -> character table: which character a physical key types in every layout of a set.
//!
//! Asking Windows (`ToUnicodeEx`) costs about 2.5 microseconds per key; a table lookup costs almost
//! nothing, so the detectors work with this table (measured, see `docs/auto-switching-architecture.md`).

use lite_switcher_core::word::KeyStroke;

/// Scancodes of the main block are below this value.
pub const SCANCODES: usize = 128;

/// What a key is, looking at all the layouts of a set.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyClass {
    /// Types nothing in any layout.
    None,
    /// A letter in every layout that types something.
    Letter,
    /// Never a letter (space, digits, `!`, `/`, ...): ends a word in every layout.
    Separator,
    /// A letter in some layouts and punctuation in others (`,` is `б` in Russian).
    Ambiguous,
}

/// Anything that can say what a key types in a layout of the set (the table, a test fake).
pub trait KeyRenderer {
    fn layout_count(&self) -> usize;
    /// The character of the key in the layout with this index; `None` if the key types nothing there.
    fn render(&self, key: &KeyStroke, layout: usize) -> Option<char>;
    fn class(&self, key: &KeyStroke) -> KeyClass;
}

/// `[layout][shift][scancode]` -> character (`'\0'` = nothing) and the class of every key.
#[derive(Debug, Clone)]
pub struct RenderTable {
    ids: Vec<String>,
    chars: Vec<[[char; SCANCODES]; 2]>,
    classes: [[KeyClass; SCANCODES]; 2],
}

impl RenderTable {
    /// Builds the table for the layouts `ids`. `typed(layout, scancode, shift)` returns the character
    /// of the key (the Windows implementation calls `ToUnicodeEx`).
    pub fn build(ids: Vec<String>, typed: impl Fn(usize, u16, bool) -> Option<char>) -> RenderTable {
        let mut chars = vec![[['\0'; SCANCODES]; 2]; ids.len()];
        for (layout, table) in chars.iter_mut().enumerate() {
            for (shift, row) in table.iter_mut().enumerate() {
                for (scancode, slot) in row.iter_mut().enumerate() {
                    if let Some(c) = typed(layout, scancode as u16, shift == 1) {
                        *slot = c;
                    }
                }
            }
        }
        let mut classes = [[KeyClass::None; SCANCODES]; 2];
        for (shift, row) in classes.iter_mut().enumerate() {
            for (scancode, slot) in row.iter_mut().enumerate() {
                let typed: Vec<char> = chars.iter().map(|t| t[shift][scancode]).filter(|c| *c != '\0').collect();
                let letters = typed.iter().filter(|c| c.is_alphabetic()).count();
                *slot = match (typed.len(), letters) {
                    (0, _) => KeyClass::None,
                    (n, l) if n == l => KeyClass::Letter,
                    (_, 0) => KeyClass::Separator,
                    _ => KeyClass::Ambiguous,
                };
            }
        }
        RenderTable { ids, chars, classes }
    }

    pub fn layout_ids(&self) -> &[String] {
        &self.ids
    }

    pub fn layout_index(&self, id: &str) -> Option<usize> {
        self.ids.iter().position(|i| i.eq_ignore_ascii_case(id))
    }

    /// Renders a whole word in one layout; `None` if some key types nothing there.
    pub fn render_word(&self, keys: &[KeyStroke], layout: usize) -> Option<String> {
        keys.iter().map(|k| self.render(k, layout)).collect()
    }
}

impl KeyRenderer for RenderTable {
    fn layout_count(&self) -> usize {
        self.ids.len()
    }

    fn render(&self, key: &KeyStroke, layout: usize) -> Option<char> {
        let scancode = key.scancode as usize;
        if scancode >= SCANCODES {
            return None;
        }
        let table = self.chars.get(layout)?;
        let base = table[0][scancode];
        // Caps Lock turns the case of letters, like Shift does (Shift + Caps Lock cancel out)
        let shift = key.shift ^ (key.caps && base.is_alphabetic());
        match table[usize::from(shift)][scancode] {
            '\0' => None,
            c => Some(c),
        }
    }

    fn class(&self, key: &KeyStroke) -> KeyClass {
        let scancode = key.scancode as usize;
        if scancode >= SCANCODES {
            return KeyClass::None;
        }
        self.classes[usize::from(key.shift)][scancode]
    }
}

/// A small keyboard for the tests of this and the other modules: layouts `en`, `ru`, `uk` with the
/// letter keys of the QWERTY rows plus the punctuation keys that are letters in Russian/Ukrainian.
#[cfg(test)]
pub(crate) mod fake {
    use super::*;

    // physical keys by their US keycap; scancodes follow the real keyboard
    const KEYS: &str = "qwertyuiop[]asdfghjkl;'zxcvbnm,./`1234567890-= ";
    const SCAN: [u16; 48] = [
        0x10, 0x11, 0x12, 0x13, 0x14, 0x15, 0x16, 0x17, 0x18, 0x19, 0x1A, 0x1B, 0x1E, 0x1F, 0x20, 0x21, 0x22,
        0x23, 0x24, 0x25, 0x26, 0x27, 0x28, 0x2C, 0x2D, 0x2E, 0x2F, 0x30, 0x31, 0x32, 0x33, 0x34, 0x35, 0x29,
        0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0A, 0x0B, 0x0C, 0x0D, 0x39, 0x00,
    ];
    const EN_SHIFT: &str = "QWERTYUIOP{}ASDFGHJKL:\"ZXCVBNM<>?~!@#$%^&*()_+ ";
    const RU: &str = "йцукенгшщзхъфывапролджэячсмитьбю.ё1234567890-= ";
    const RU_SHIFT: &str = "ЙЦУКЕНГШЩЗХЪФЫВАПРОЛДЖЭЯЧСМИТЬБЮ,Ё!\"№;%:?*()_+ ";
    const UK: &str = "йцукенгшщзхїфівапролджєячсмитьбю.ґ1234567890-= ";
    const UK_SHIFT: &str = "ЙЦУКЕНГШЩЗХЇФІВАПРОЛДЖЄЯЧСМИТЬБЮ,Ґ!\"№;%:?*()_+ ";

    fn row(layout: &str, shift: bool) -> &'static str {
        match (layout, shift) {
            ("en", false) => KEYS,
            ("en", true) => EN_SHIFT,
            ("ru", false) => RU,
            ("ru", true) => RU_SHIFT,
            ("uk", false) => UK,
            _ => UK_SHIFT,
        }
    }

    /// The table for the given layout ids (`en`, `ru`, `uk`).
    pub fn table(ids: &[&str]) -> RenderTable {
        let owned: Vec<String> = ids.iter().map(|s| s.to_string()).collect();
        let names = owned.clone();
        RenderTable::build(owned, move |layout, scancode, shift| {
            let pos = SCAN.iter().position(|&s| s == scancode && s != 0)?;
            row(&names[layout], shift).chars().nth(pos)
        })
    }

    /// The keys that type `text` in the layout `layout` (letters and the punctuation of the rows).
    pub fn keys_for(text: &str, layout: &str) -> Vec<KeyStroke> {
        text.chars()
            .map(|c| {
                for shift in [false, true] {
                    if let Some(pos) = row(layout, shift).chars().position(|x| x == c) {
                        return KeyStroke { scancode: SCAN[pos], shift, caps: false };
                    }
                }
                panic!("{c:?} cannot be typed in {layout}");
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::fake::{keys_for, table};
    use super::*;

    #[test]
    fn renders_the_same_keys_in_every_layout() {
        let t = table(&["en", "ru", "uk"]);
        let keys = keys_for("sdf", "en");
        assert_eq!(t.render_word(&keys, 0).as_deref(), Some("sdf"));
        assert_eq!(t.render_word(&keys, 1).as_deref(), Some("ыва"));
        assert_eq!(t.render_word(&keys, 2).as_deref(), Some("іва"));
        assert_eq!(t.render_word(&keys_for("ghbdtn", "en"), 1).as_deref(), Some("привет"));
        assert_eq!(t.render_word(&keys_for("Привет", "ru"), 0).as_deref(), Some("Ghbdtn"));
        assert_eq!(t.layout_index("RU"), Some(1));
    }

    #[test]
    fn shift_and_caps_lock_change_the_case() {
        let t = table(&["en", "ru"]);
        let w = KeyStroke { scancode: 0x11, shift: false, caps: false };
        assert_eq!(t.render(&w, 0), Some('w'));
        assert_eq!(t.render(&KeyStroke { shift: true, ..w }, 1), Some('Ц'));
        assert_eq!(t.render(&KeyStroke { caps: true, ..w }, 1), Some('Ц'));
        assert_eq!(t.render(&KeyStroke { shift: true, caps: true, ..w }, 1), Some('ц'), "they cancel out");
        // Caps Lock does not touch digits or punctuation
        let one = KeyStroke { scancode: 0x02, shift: false, caps: true };
        assert_eq!(t.render(&one, 0), Some('1'));
        assert_eq!(t.render(&KeyStroke { scancode: 0x7F, shift: false, caps: false }, 0), None);
        assert_eq!(t.render(&KeyStroke { scancode: 0x200, shift: false, caps: false }, 0), None);
    }

    #[test]
    fn classifies_the_keys() {
        let t = table(&["en", "ru", "uk"]);
        let class = |c: char, layout: &str| t.class(&keys_for(&c.to_string(), layout)[0]);
        assert_eq!(class('q', "en"), KeyClass::Letter);
        assert_eq!(class(' ', "en"), KeyClass::Separator);
        assert_eq!(class('1', "en"), KeyClass::Separator);
        assert_eq!(class('/', "en"), KeyClass::Separator, "'/' is '.' in ru: never a letter");
        assert_eq!(class(',', "en"), KeyClass::Ambiguous, "'б' in ru");
        assert_eq!(class('.', "en"), KeyClass::Ambiguous, "'ю' in ru");
        assert_eq!(class(';', "en"), KeyClass::Ambiguous, "'ж' in ru");
        assert_eq!(class('[', "en"), KeyClass::Ambiguous, "'х' in ru");
        let shifted_slash = KeyStroke { scancode: 0x35, shift: true, caps: false };
        assert_eq!(t.class(&shifted_slash), KeyClass::Separator, "'?' in en, ',' in ru");
        // with only English in the set, the comma is a plain separator
        let en = table(&["en"]);
        assert_eq!(en.class(&keys_for(",", "en")[0]), KeyClass::Separator);
    }
}
