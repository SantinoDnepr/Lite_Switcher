//! The programming language detector (for 1C and, later, other languages).
//!
//! Only the dictionary decides: the typed keys are read in every pair of layouts of the pack, as a
//! whole word and with a change of the layout inside the word (`ЕстьNull`, `#Область`), and the
//! reading that is a word of the language wins. No trigram statistics: a program is full of
//! identifiers that are not words of any language.
//!
//! A word that is already right in the current layout is kept. A hybrid word (a part in each of two
//! layouts) is fixed without changing the layout of the window.

use std::sync::Arc;

use lite_switcher_core::word::KeyStroke;

use crate::format::flags;
use crate::literary::Context;
use crate::pack::{id_matches, LoadedPack};
use crate::render::{KeyClass, KeyRenderer, RenderTable};

/// The most keys at each end of the token that are tried as punctuation.
const MAX_STRIP: usize = 2;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeepReason {
    /// The current layout is not one of the layouts of the language.
    NoPack,
    TooShort,
    /// The token is a word of the language in the current layout already.
    Dictionary,
    /// No reading of the keys is a word of the language.
    NoCandidate,
    /// Several different readings are equally good.
    Ambiguous,
}

/// How a reading was recognized (the better ones first).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Match {
    /// The dictionary has exactly this word.
    Exact,
    /// A sigil (`#`, `&`) and a word of the dictionary: `&НачалоПериода`.
    Sigil,
    /// A word of the dictionary and a Latin segment (`ЗаписьXML`).
    Composition,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Replace {
    /// The token as it has to be typed (the case of the typed keys is kept).
    pub text: String,
    /// The layout to switch the window to; `None` = keep the current one.
    pub layout: Option<usize>,
    pub kind: Match,
    /// Two parts in different layouts.
    pub hybrid: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    Keep(KeepReason),
    Replace(Replace),
}

/// The keys read in some layouts.
struct Reading {
    /// Everything, with the punctuation around the token.
    text: String,
    /// The token without the punctuation: this is what the dictionary is asked about.
    core: String,
    /// The layouts that type letters in the token.
    letter_layouts: Vec<usize>,
}

pub struct ProgrammingDetector {
    pack: Arc<LoadedPack>,
    /// The layouts of the language in the table (a layout for every entry of `layout_require`).
    layouts: Vec<usize>,
    segments: Vec<String>,
    sigils: Vec<char>,
    /// The keys that type a sigil in a layout of the language.
    sigil_keys: Vec<(u16, bool)>,
}

impl ProgrammingDetector {
    pub fn new(pack: Arc<LoadedPack>, table: &RenderTable) -> ProgrammingDetector {
        let params = &pack.manifest.programming;
        let lower = |s: &String| if params.case_sensitive { s.clone() } else { s.to_lowercase() };
        let layouts: Vec<usize> = pack
            .manifest
            .layout_require
            .iter()
            .filter_map(|prefix| table.layout_ids().iter().position(|id| id_matches(id, prefix)))
            .collect();
        let sigils: Vec<char> = params.sigils.iter().filter_map(|s| s.chars().next()).collect();
        let mut sigil_keys = Vec::new();
        for shift in [false, true] {
            for scancode in 0..crate::render::SCANCODES as u16 {
                let key = KeyStroke { scancode, shift, caps: false };
                if layouts.iter().any(|&l| table.render(&key, l).is_some_and(|c| sigils.contains(&c))) {
                    sigil_keys.push((scancode, shift));
                }
            }
        }
        ProgrammingDetector { segments: params.latin_segments.iter().map(lower).collect(), sigils, sigil_keys, layouts, pack }
    }

    /// Is the layout (index in the table) one of the layouts of the language?
    pub fn serves(&self, layout: usize) -> bool {
        self.layouts.contains(&layout)
    }

    /// Does the key type a sigil (`#`, `&`) in a layout of the language? A sigil starts a token.
    pub fn is_sigil_key(&self, key: &KeyStroke) -> bool {
        self.sigil_keys.contains(&(key.scancode, key.shift))
    }

    fn normalize(&self, text: &str) -> String {
        if self.pack.manifest.programming.case_sensitive { text.to_string() } else { text.to_lowercase() }
    }

    /// Is `word` in the dictionary? A word of up to three letters has to be a keyword of the language:
    /// short identifiers are everywhere ("reg", "key") and are also ordinary words of other languages.
    fn known(&self, word: &str) -> bool {
        match self.pack.words.get(word) {
            Some(value) => {
                word.chars().count() > 3 || value & (flags::KEYWORD | flags::DIRECTIVE | flags::PREPROCESSOR) != 0
            }
            None => false,
        }
    }

    /// Is the text a word of the language?
    fn recognize(&self, text: &str) -> Option<Match> {
        let key = self.normalize(text);
        if self.known(&key) {
            return Some(Match::Exact);
        }
        let mut chars = key.chars();
        if chars.next().is_some_and(|c| self.sigils.contains(&c)) {
            let body = chars.as_str();
            if !body.is_empty() && self.known(body) {
                return Some(Match::Sigil);
            }
        }
        for segment in &self.segments {
            let composed = |rest: &str| rest.chars().count() >= 3 && self.known(rest);
            if let Some(rest) = key.strip_suffix(segment.as_str()) {
                if composed(rest) {
                    return Some(Match::Composition);
                }
            }
            if let Some(rest) = key.strip_prefix(segment.as_str()) {
                if composed(rest) {
                    return Some(Match::Composition);
                }
            }
        }
        None
    }

    pub fn decide(&self, table: &RenderTable, ctx: &Context) -> Verdict {
        let keys = ctx.keys;
        let n = keys.len();
        if !self.serves(ctx.current) {
            return Verdict::Keep(KeepReason::NoPack);
        }
        if n < self.pack.manifest.programming.min_token_len {
            return Verdict::Keep(KeepReason::TooShort);
        }
        // a token of a program does not start with a key that is a letter in some layout: only the
        // trailing keys are tried as punctuation (`Если,`); the core has to be a whole token
        let ambiguous = |k: &&KeyStroke| table.class(k) == KeyClass::Ambiguous;
        let trail_max = keys.iter().rev().take_while(ambiguous).count().min(MAX_STRIP);
        let min_len = self.pack.manifest.programming.min_token_len;
        let strips: Vec<(usize, usize)> = (0..=trail_max).map(|t| (0, t)).filter(|(_, t)| n - t >= min_len).collect();

        // 1. the token in the current layout
        for &(lead, trail) in &strips {
            if let Some(reading) = self.reading(table, keys, lead, trail, &[(ctx.current, 0)]) {
                if self.recognize(&reading.core).is_some() {
                    return Verdict::Keep(KeepReason::Dictionary);
                }
            }
        }

        // 2. the readings that are words of the language
        struct Found {
            /// The whole text, with the punctuation around the token.
            text: String,
            kind: Match,
            splits: usize,
            letter_layouts: Vec<usize>,
        }
        let mut found: Vec<Found> = Vec::new();
        for &(lead, trail) in &strips {
            let core = n - lead - trail;
            for &a in &self.layouts {
                for &b in &self.layouts {
                    // `a` for the first part of the core, `b` for the rest; a == b is the whole word
                    for split in if a == b { vec![0] } else { (1..core).collect() } {
                        let plan = [(a, 0), (b, split)];
                        let Some(reading) = self.reading(table, keys, lead, trail, &plan) else { continue };
                        if let Some(kind) = self.recognize(&reading.core) {
                            found.push(Found {
                                text: reading.text,
                                kind,
                                splits: usize::from(a != b),
                                letter_layouts: reading.letter_layouts,
                            });
                        }
                    }
                }
            }
        }
        let Some(best) = found.iter().map(|f| (f.kind, f.splits)).min() else {
            return Verdict::Keep(KeepReason::NoCandidate);
        };
        let top: Vec<&Found> = found.iter().filter(|f| (f.kind, f.splits) == best).collect();
        let first = top[0];
        if top.iter().any(|f| f.text != first.text) {
            return Verdict::Keep(KeepReason::Ambiguous);
        }
        // the layout: unchanged if it is the layout of a part of the word (sigils and punctuation
        // do not count), otherwise the layout of the letters
        let layout = if first.letter_layouts.contains(&ctx.current) {
            None
        } else {
            first.letter_layouts.first().copied()
        };
        let current_text = table.render_word(keys, ctx.current);
        if layout.is_none() && current_text.as_deref() == Some(first.text.as_str()) {
            return Verdict::Keep(KeepReason::Dictionary);
        }
        Verdict::Replace(Replace {
            text: first.text.clone(),
            layout,
            kind: first.kind,
            hybrid: first.letter_layouts.len() > 1,
        })
    }

    /// Renders the keys: the first `lead` keys, the core and the last `trail` keys. `plan` lists
    /// `(layout, first core index)` pairs: the core from that index on is rendered in that layout.
    /// Returns the readings; `None` if a key types nothing, or a stripped key is a letter.
    fn reading(
        &self,
        table: &RenderTable,
        keys: &[KeyStroke],
        lead: usize,
        trail: usize,
        plan: &[(usize, usize)],
    ) -> Option<Reading> {
        let n = keys.len();
        let end = n - trail;
        let layout_of = |core_index: usize| plan.iter().rev().find(|(_, from)| core_index >= *from).map_or(plan[0].0, |p| p.0);
        let mut text = String::new();
        let mut core = String::new();
        let mut letter_layouts: Vec<usize> = Vec::new();
        for (i, key) in keys.iter().enumerate() {
            let in_core = i >= lead && i < end;
            let layout = if in_core { layout_of(i - lead) } else if i < lead { layout_of(0) } else { layout_of(end - lead - 1) };
            let c = table.render(key, layout)?;
            if !in_core && c.is_alphabetic() {
                return None;
            }
            if c.is_alphabetic() && !letter_layouts.contains(&layout) {
                letter_layouts.push(layout);
            }
            text.push(c);
            if in_core {
                core.push(c);
            }
        }
        Some(Reading { text, core, letter_layouts })
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::format::{flags, Words};
    use crate::manifest::{tests::PROGRAMMING, Manifest};
    use crate::render::presets::{keys_for, table};

    pub fn pack(extra: &[&str]) -> Arc<LoadedPack> {
        let text = PROGRAMMING.replace("latin_segments = Null, XML", "latin_segments = Null, XML, JSON");
        let manifest = Manifest::parse(&text, Some("1c")).unwrap();
        let mut words: Vec<(String, u32)> = [
            "процедура", "procedure", "конецпроцедуры", "функция", "если", "тогда", "иначе", "конецесли", "возврат",
            "новый", "запрос", "выполнить", "#область", "#region", "&наклиенте", "&atclient", "&насервере",
            "естьnull", "null", "начало периода", "началопериода", "xml", "запись",
        ]
        .iter()
        .map(|w| (w.to_string(), flags::KEYWORD))
        .collect();
        words.extend(extra.iter().map(|w| (w.to_string(), 1)));
        let bytes = Words::build(words).unwrap();
        Arc::new(LoadedPack {
            manifest,
            words: Words::from_bytes(bytes).unwrap(),
            ngram: None,
            exceptions: Default::default(),
        })
    }

    const EN: usize = 0;
    const RU: usize = 1;

    fn setup() -> (ProgrammingDetector, RenderTable) {
        let t = table(&["en", "ru"]);
        (ProgrammingDetector::new(pack(&[]), &t), t)
    }

    fn decide(d: &ProgrammingDetector, t: &RenderTable, text: &str, typed_in: &str, current: usize) -> Verdict {
        let keys = keys_for(text, typed_in);
        d.decide(t, &Context { keys: &keys, current, previous: None })
    }

    /// The keys of text typed in parts: `(text, the layout whose keys type it)`.
    fn mixed(parts: &[(&str, &str)]) -> Vec<KeyStroke> {
        parts.iter().flat_map(|(text, layout)| keys_for(text, layout)).collect()
    }

    fn replace(v: Verdict) -> Replace {
        match v {
            Verdict::Replace(r) => r,
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn a_keyword_typed_in_the_wrong_layout_is_restored_and_the_layout_follows() {
        let (d, t) = setup();
        let r = replace(decide(&d, &t, "Процедура", "ru", EN));
        assert_eq!((r.text.as_str(), r.layout, r.hybrid), ("Процедура", Some(RU), false));
        let r = replace(decide(&d, &t, "Procedure", "en", RU));
        assert_eq!((r.text.as_str(), r.layout), ("Procedure", Some(EN)));
        // the case of the keys is kept
        let r = replace(decide(&d, &t, "КОНЕЦПРОЦЕДУРЫ", "ru", EN));
        assert_eq!(r.text, "КОНЕЦПРОЦЕДУРЫ");
    }

    #[test]
    fn a_keyword_that_is_right_already_is_kept() {
        let (d, t) = setup();
        assert_eq!(decide(&d, &t, "Процедура", "ru", RU), Verdict::Keep(KeepReason::Dictionary));
        assert_eq!(decide(&d, &t, "Procedure", "en", EN), Verdict::Keep(KeepReason::Dictionary));
    }

    #[test]
    fn the_sigil_typed_in_the_russian_layout_is_fixed_and_the_layout_stays() {
        let (d, t) = setup();
        // "№Область": the key of `#` types `№` in the Russian layout
        let r = replace(decide(&d, &t, "№Область", "ru", RU));
        assert_eq!((r.text.as_str(), r.layout, r.hybrid), ("#Область", None, false), "the sigil is not a letter");
        // "?НаКлиенте": Shift+7 types `?` in Russian and `&` in English
        let r = replace(decide(&d, &t, "?НаКлиенте", "ru", RU));
        assert_eq!((r.text.as_str(), r.layout), ("&НаКлиенте", None));
    }

    #[test]
    fn a_directive_typed_entirely_in_english_switches_to_russian() {
        let (d, t) = setup();
        // the keys of `&` in English and of `НаКлиенте` in Russian, typed with the English layout active
        let keys = mixed(&[("&", "en"), ("НаКлиенте", "ru")]);
        let r = replace(d.decide(&t, &Context { keys: &keys, current: EN, previous: None }));
        assert_eq!((r.text.as_str(), r.layout), ("&НаКлиенте", Some(RU)), "the letters are Russian, the sigil does not count");
    }

    #[test]
    fn a_word_with_a_latin_part_keeps_the_layout() {
        let (d, t) = setup();
        // `ЕстьNull` typed in the Russian layout shows `ЕстьТгдд`
        let keys: Vec<KeyStroke> = keys_for("Есть", "ru").into_iter().chain(keys_for("Null", "en")).collect();
        let r = replace(d.decide(&t, &Context { keys: &keys, current: RU, previous: None }));
        assert_eq!((r.text.as_str(), r.layout, r.hybrid), ("ЕстьNull", None, true));
        let r = replace(d.decide(&t, &Context { keys: &keys, current: EN, previous: None }));
        assert_eq!((r.text.as_str(), r.layout), ("ЕстьNull", None), "also when the window is in English");
    }

    #[test]
    fn a_dictionary_word_and_a_latin_segment_make_a_word() {
        let (d, t) = setup();
        // "ЗаписьXML" is not in the dictionary but "запись" + "xml" is a composition
        let keys: Vec<KeyStroke> = keys_for("Запись", "ru").into_iter().chain(keys_for("XML", "en")).collect();
        let r = replace(d.decide(&t, &Context { keys: &keys, current: RU, previous: None }));
        assert_eq!((r.text.as_str(), r.kind), ("ЗаписьXML", Match::Composition));
    }

    #[test]
    fn a_sigil_before_a_known_word_is_accepted() {
        // `&НачалоПериода` is a query parameter: only the word is in the dictionary
        let t = table(&["en", "ru"]);
        let d = ProgrammingDetector::new(pack(&["началопериода"]), &t);
        let keys = mixed(&[("&", "en"), ("НачалоПериода", "ru")]);
        let r = replace(d.decide(&t, &Context { keys: &keys, current: EN, previous: None }));
        assert_eq!((r.text.as_str(), r.kind, r.layout), ("&НачалоПериода", Match::Sigil, Some(RU)));
    }

    #[test]
    fn unknown_words_and_short_tokens_are_left_alone() {
        let (d, t) = setup();
        assert_eq!(decide(&d, &t, "Привет", "ru", EN), Verdict::Keep(KeepReason::NoCandidate));
        assert_eq!(decide(&d, &t, "ща", "ru", EN), Verdict::Keep(KeepReason::TooShort));
        let t3 = table(&["en", "ru", "uk"]);
        let d3 = ProgrammingDetector::new(pack(&[]), &t3);
        let keys = keys_for("Процедура", "ru");
        // the Ukrainian layout is not a layout of the language
        assert_eq!(d3.decide(&t3, &Context { keys: &keys, current: 2, previous: None }), Verdict::Keep(KeepReason::NoPack));
    }

    #[test]
    fn punctuation_around_a_keyword_is_not_part_of_it() {
        let (d, t) = setup();
        // an English word and a comma typed with the Russian layout: the comma key is `б` there
        let keys = keys_for("Procedure,", "en");
        let r = replace(d.decide(&t, &Context { keys: &keys, current: RU, previous: None }));
        assert_eq!((r.text.as_str(), r.layout), ("Procedure,", Some(EN)), "the comma is typed in the layout of the word");
        // the same keys are a Russian word followed by a letter: not a word of the language
        let keys = mixed(&[("Если", "ru"), (",", "en")]);
        assert!(matches!(d.decide(&t, &Context { keys: &keys, current: EN, previous: None }), Verdict::Keep(_)));
    }

    #[test]
    fn sigil_keys_are_known() {
        let (d, t) = setup();
        assert!(d.is_sigil_key(&keys_for("#", "en")[0]));
        assert!(d.is_sigil_key(&keys_for("&", "en")[0]));
        assert!(!d.is_sigil_key(&keys_for("q", "en")[0]));
        let _ = t;
    }
}
