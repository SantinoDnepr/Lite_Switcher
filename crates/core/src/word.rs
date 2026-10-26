//! The buffer of the last typed word (as physical keys) and the conversion cycle state.

use crate::layout::Layout;

/// A physical key press that produced a printable character.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KeyStroke {
    pub scancode: u16,
    pub shift: bool,
    pub caps: bool,
}

/// What has to be done in the focused window to convert the word.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Replacement {
    /// Number of characters to erase (the word plus the keys typed after it).
    pub backspaces: usize,
    /// Text to type instead (the word in the target layout plus the keys typed after it).
    pub text: String,
    pub target: Layout,
}

/// Bits of `WordBuffer::taint_bits`: the keys that were pressed while the word was typed.
pub mod taint {
    pub const BACKSPACE: u8 = 1;
    pub const LEFT: u8 = 2;
    pub const RIGHT: u8 = 4;
    pub const UP: u8 = 8;
    pub const DOWN: u8 = 16;
    pub const DELETE: u8 = 32;
    pub const CHANGE_LAYOUT: u8 = 64;
}

#[derive(Debug, Default)]
pub struct WordBuffer {
    keys: Vec<KeyStroke>,
    /// Number of characters of the word currently shown in the window.
    shown_len: usize,
    /// Keys typed after the word (spaces, punctuation): they stay with the word when it is converted.
    trailing: Vec<KeyStroke>,
    /// Active conversion cycle: layouts to visit and the index of the layout shown now.
    cycle: Option<(Vec<Layout>, usize)>,
    /// Layout (raw HKL) of the focused window when the first key of the word was typed.
    start_hkl: Option<isize>,
    /// Keys (`taint` bits) pressed while the word was typed: such a word is "unclean" and the
    /// automatic switching skips it.
    taint: u8,
}

impl WordBuffer {
    pub fn clear(&mut self) {
        *self = WordBuffer::default();
    }

    pub fn is_empty(&self) -> bool {
        self.keys.is_empty()
    }

    pub fn keys(&self) -> &[KeyStroke] {
        &self.keys
    }

    pub fn trailing(&self) -> &[KeyStroke] {
        &self.trailing
    }

    pub fn shown_len(&self) -> usize {
        self.shown_len
    }

    /// The layout of the window when the word was started.
    pub fn start_hkl(&self) -> Option<isize> {
        self.start_hkl
    }

    /// The layout of the window changed on purpose (by the program itself): not a "change layout" key.
    pub fn set_start_hkl(&mut self, hkl: isize) {
        self.start_hkl = Some(hkl);
    }

    /// The word starts over in another layout (the user changed it and that is allowed).
    pub fn restart_in(&mut self, hkl: isize) {
        let keep = self.taint;
        self.clear();
        self.taint = keep;
        self.start_hkl = Some(hkl);
    }

    /// A printable key was typed in the window whose layout is `hkl`.
    pub fn push_in(&mut self, key: KeyStroke, hkl: isize) {
        if !self.trailing.is_empty() {
            self.clear();
        }
        if self.keys.is_empty() && self.start_hkl.is_none() {
            self.start_hkl = Some(hkl);
        }
        self.keys.push(key);
        self.shown_len += 1;
        self.cycle = None;
    }

    /// A printable key was typed (the layout of the window is not tracked).
    pub fn push(&mut self, key: KeyStroke) {
        self.push_in(key, 0);
        self.start_hkl = None;
    }

    /// A key that ends the word was typed (space, punctuation). With `keep_trailing` the word stays
    /// convertible together with such keys, otherwise the word ends.
    pub fn trailing_key(&mut self, key: KeyStroke, keep_trailing: bool) {
        if self.keys.is_empty() || !keep_trailing {
            self.clear();
        } else {
            self.trailing.push(key);
            self.cycle = None;
        }
    }

    /// A space was typed.
    pub fn space(&mut self, keep_trailing: bool) {
        self.trailing_key(KeyStroke { scancode: 0x39, shift: false, caps: false }, keep_trailing);
    }

    pub fn backspace(&mut self) {
        if self.trailing.pop().is_none() && self.keys.pop().is_some() {
            self.shown_len = self.shown_len.saturating_sub(1);
        }
        self.cycle = None;
    }

    /// A key with an unwanted effect was pressed: the word is "unclean" until it ends.
    pub fn taint(&mut self, bits: u8) {
        self.taint |= bits;
    }

    pub fn taint_bits(&self) -> u8 {
        self.taint
    }

    pub fn is_tainted(&self) -> bool {
        self.taint != 0
    }

    /// The word was replaced automatically: it is now shown with `shown_len` characters followed by
    /// `trailing` keys, in the layout `pos` of `seq` (the cycle of the hotkey continues from there).
    pub fn after_auto_replacement(&mut self, shown_len: usize, trailing: Vec<KeyStroke>, seq: Vec<Layout>, pos: usize) {
        self.shown_len = shown_len;
        self.trailing = trailing;
        self.cycle = Some((seq, pos));
        self.taint = 0;
    }

    /// Computes the next conversion of the word. `make_seq` is called only when a new cycle starts
    /// and returns the layouts to visit (see `layout::cycle_sequence`); `render` maps a key to a
    /// character in a layout. The buffer state is advanced as if the replacement was performed.
    pub fn next_replacement(
        &mut self,
        make_seq: impl FnOnce() -> Vec<Layout>,
        render: impl Fn(&KeyStroke, &Layout) -> Option<char>,
    ) -> Option<Replacement> {
        if self.keys.is_empty() {
            return None;
        }
        let (seq, pos) = match self.cycle.take() {
            Some(c) => c,
            None => {
                let seq = make_seq();
                let last = seq.len().checked_sub(1)?;
                (seq, last)
            }
        };
        if seq.len() < 2 {
            return None;
        }
        let next = (pos + 1) % seq.len();
        let target = seq[next].clone();
        let word: String = self.keys.iter().filter_map(|k| render(k, &target)).collect();
        let tail: String = self.trailing.iter().filter_map(|k| render(k, &target)).collect();
        let replacement = Replacement {
            backspaces: self.shown_len + self.trailing.len(),
            text: format!("{word}{tail}"),
            target,
        };
        self.shown_len = word.chars().count();
        self.cycle = Some((seq, next));
        Some(replacement)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layout::cycle_sequence;

    fn l(id: &str, hkl: isize) -> Layout {
        Layout { hkl, id: id.into(), name: id.into() }
    }

    fn layouts() -> Vec<Layout> {
        vec![l("en-US", 1), l("ru-RU", 2), l("uk-UA", 3)]
    }

    // fake layouts: scancode 1 -> a/b/c, scancode 2 -> x/y/z by hkl, scancode 0x39 = space,
    // scancode 0x35 = '/' in layout 1 and '.' in layout 2 and 3 (a punctuation key)
    fn render(k: &KeyStroke, l: &Layout) -> Option<char> {
        match k.scancode {
            0x39 => Some(' '),
            0x35 => Some(if l.hkl == 1 { '/' } else { '.' }),
            sc => char::from_u32((if sc == 1 { 'a' } else { 'x' }) as u32 + (l.hkl as u32 - 1)),
        }
    }

    fn key(scancode: u16) -> KeyStroke {
        KeyStroke { scancode, shift: false, caps: false }
    }

    fn typed(buf: &mut WordBuffer, n: u16) {
        for i in 0..n {
            buf.push(key(1 + i % 2));
        }
    }

    #[test]
    fn cycles_through_other_layouts_then_back() {
        let mut buf = WordBuffer::default();
        typed(&mut buf, 3); // typed in ru: shown as "byb"
        let all = layouts();
        let seq = || cycle_sequence(&all, Some("ru-RU"));
        // list is en, ru, uk: from ru the next one is uk, then en, then back to ru
        let r1 = buf.next_replacement(seq, render).unwrap();
        assert_eq!((r1.target.id.as_str(), r1.text.as_str(), r1.backspaces), ("uk-UA", "czc", 3));
        let r2 = buf.next_replacement(|| unreachable!(), render).unwrap();
        assert_eq!((r2.target.id.as_str(), r2.text.as_str(), r2.backspaces), ("en-US", "axa", 3));
        let r3 = buf.next_replacement(|| unreachable!(), render).unwrap();
        assert_eq!(r3.target.id, "ru-RU");
        let r4 = buf.next_replacement(|| unreachable!(), render).unwrap();
        assert_eq!(r4.target.id, "uk-UA");
    }

    #[test]
    fn trailing_spaces_are_kept() {
        let mut buf = WordBuffer::default();
        typed(&mut buf, 2);
        buf.space(true);
        buf.space(true);
        let all = layouts();
        let r = buf.next_replacement(|| cycle_sequence(&all, Some("en-US")), render).unwrap();
        assert_eq!(r.backspaces, 4);
        assert_eq!(r.text, "by  ");
    }

    #[test]
    fn trailing_punctuation_is_rendered_in_the_target_layout() {
        let mut buf = WordBuffer::default();
        typed(&mut buf, 2); // typed in en
        buf.trailing_key(key(0x35), true); // '/' in en
        let all = layouts();
        let r = buf.next_replacement(|| cycle_sequence(&all, Some("en-US")), render).unwrap();
        assert_eq!((r.target.id.as_str(), r.text.as_str(), r.backspaces), ("ru-RU", "by.", 3));
    }

    #[test]
    fn typing_resets_cycle_but_keeps_word() {
        let mut buf = WordBuffer::default();
        typed(&mut buf, 2);
        let all = layouts();
        buf.next_replacement(|| cycle_sequence(&all, Some("en-US")), render).unwrap();
        buf.push(key(1));
        let r = buf.next_replacement(|| cycle_sequence(&all, Some("ru-RU")), render).unwrap();
        assert_eq!(r.backspaces, 3);
        // the word is now shown in ru, so the next layout after ru in the list is uk
        assert_eq!(r.target.id, "uk-UA");
    }

    #[test]
    fn space_without_keep_ends_word_and_backspace_edits() {
        let mut buf = WordBuffer::default();
        typed(&mut buf, 2);
        buf.backspace();
        let all = layouts();
        let r = buf.next_replacement(|| cycle_sequence(&all, Some("en-US")), render).unwrap();
        assert_eq!(r.backspaces, 1);

        let mut buf = WordBuffer::default();
        typed(&mut buf, 2);
        buf.space(false);
        assert!(buf.is_empty());
        assert!(buf.next_replacement(layouts, render).is_none());
    }

    #[test]
    fn single_layout_does_nothing() {
        let mut buf = WordBuffer::default();
        typed(&mut buf, 1);
        assert!(buf.next_replacement(|| vec![l("en-US", 1)], render).is_none());
    }

    #[test]
    fn the_start_layout_is_remembered_per_word() {
        let mut buf = WordBuffer::default();
        buf.push_in(key(1), 77);
        buf.push_in(key(2), 88);
        assert_eq!(buf.start_hkl(), Some(77), "the layout of the first key");
        buf.space(true);
        buf.push_in(key(1), 99);
        assert_eq!(buf.start_hkl(), Some(99), "a new word after a space starts a new episode");
        assert_eq!(buf.keys().len(), 1);
        buf.restart_in(5);
        assert!(buf.is_empty() && buf.start_hkl() == Some(5));
    }

    #[test]
    fn taint_lasts_until_the_word_ends() {
        let mut buf = WordBuffer::default();
        buf.push_in(key(1), 1);
        buf.taint(taint::BACKSPACE);
        assert!(buf.is_tainted() && buf.taint_bits() == taint::BACKSPACE);
        buf.push_in(key(2), 1);
        assert!(buf.is_tainted(), "typing more does not clean the word");
        buf.space(true);
        assert!(buf.is_tainted(), "the word is still the same until the next one starts");
        buf.push_in(key(1), 1);
        assert!(!buf.is_tainted(), "the next word is clean");

        // an arrow clears the buffer but the fragment typed after it stays tainted
        let mut buf = WordBuffer::default();
        buf.push_in(key(1), 1);
        buf.clear();
        buf.taint(taint::LEFT);
        buf.push_in(key(2), 1);
        assert!(buf.is_tainted());
        // without text a boundary key forgets the taint
        let mut buf = WordBuffer::default();
        buf.taint(taint::LEFT);
        buf.space(true);
        assert!(!buf.is_tainted());
    }

    #[test]
    fn the_hotkey_continues_the_cycle_after_an_automatic_replacement() {
        // "byb" was typed in en and replaced with the ru word followed by a space
        let all = layouts();
        let mut buf = WordBuffer::default();
        typed(&mut buf, 3);
        let seq = cycle_sequence(&all, Some("en-US")); // ru, uk, en
        buf.after_auto_replacement(3, vec![key(0x39)], seq, 0); // shown in ru
        let r = buf.next_replacement(|| unreachable!(), render).unwrap();
        assert_eq!((r.target.id.as_str(), r.text.as_str(), r.backspaces), ("uk-UA", "czc ", 4));
        let r = buf.next_replacement(|| unreachable!(), render).unwrap();
        assert_eq!(r.target.id, "en-US", "and back to the original layout after a full round");
    }
}
