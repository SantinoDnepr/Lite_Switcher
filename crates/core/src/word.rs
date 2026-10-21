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
    /// Number of characters to erase (the word plus trailing spaces).
    pub backspaces: usize,
    /// Text to type instead (the word in the target layout plus trailing spaces).
    pub text: String,
    pub target: Layout,
}

#[derive(Debug, Default)]
pub struct WordBuffer {
    keys: Vec<KeyStroke>,
    /// Number of characters of the word currently shown in the window.
    shown_len: usize,
    /// Spaces typed after the word.
    spaces: usize,
    /// Active conversion cycle: layouts to visit and the index of the layout shown now.
    cycle: Option<(Vec<Layout>, usize)>,
}

impl WordBuffer {
    pub fn clear(&mut self) {
        *self = WordBuffer::default();
    }

    pub fn is_empty(&self) -> bool {
        self.keys.is_empty()
    }

    /// A printable key was typed.
    pub fn push(&mut self, key: KeyStroke) {
        if self.spaces > 0 {
            self.clear();
        }
        self.keys.push(key);
        self.shown_len += 1;
        self.cycle = None;
    }

    /// Space was typed. With `keep_trailing` the word stays convertible, otherwise the word ends.
    pub fn space(&mut self, keep_trailing: bool) {
        if self.keys.is_empty() || !keep_trailing {
            self.clear();
        } else {
            self.spaces += 1;
            self.cycle = None;
        }
    }

    pub fn backspace(&mut self) {
        if self.spaces > 0 {
            self.spaces -= 1;
        } else if self.keys.pop().is_some() {
            self.shown_len = self.shown_len.saturating_sub(1);
        }
        self.cycle = None;
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
        let replacement = Replacement {
            backspaces: self.shown_len + self.spaces,
            text: format!("{word}{}", " ".repeat(self.spaces)),
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

    // fake layouts: scancode 1 -> a/b/c, scancode 2 -> x/y/z by hkl
    fn render(k: &KeyStroke, l: &Layout) -> Option<char> {
        let base = if k.scancode == 1 { 'a' } else { 'x' };
        char::from_u32(base as u32 + (l.hkl as u32 - 1))
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
        let r1 = buf.next_replacement(seq, render).unwrap();
        assert_eq!((r1.target.id.as_str(), r1.text.as_str(), r1.backspaces), ("en-US", "axa", 3));
        let r2 = buf.next_replacement(|| unreachable!(), render).unwrap();
        assert_eq!((r2.target.id.as_str(), r2.text.as_str(), r2.backspaces), ("uk-UA", "czc", 3));
        let r3 = buf.next_replacement(|| unreachable!(), render).unwrap();
        assert_eq!(r3.target.id, "ru-RU");
        let r4 = buf.next_replacement(|| unreachable!(), render).unwrap();
        assert_eq!(r4.target.id, "en-US");
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
    fn typing_resets_cycle_but_keeps_word() {
        let mut buf = WordBuffer::default();
        typed(&mut buf, 2);
        let all = layouts();
        buf.next_replacement(|| cycle_sequence(&all, Some("en-US")), render).unwrap();
        buf.push(key(1));
        let r = buf.next_replacement(|| cycle_sequence(&all, Some("ru-RU")), render).unwrap();
        assert_eq!(r.backspaces, 3);
        assert_eq!(r.target.id, "en-US");
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
}
