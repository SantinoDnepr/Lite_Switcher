//! The literary detector: decides whether the word that was just typed was typed in the wrong
//! layout (the cascade is described in `docs/auto-switching-architecture.md`, section 6).
//!
//! 1. the word is rendered in every layout that has an active pack, the current one included;
//! 2. a word that is valid in the current layout is kept: a dictionary word, an exception, an
//!    abbreviation in capitals;
//! 3. otherwise the layout in which the word looks most like a word (letter trigram costs) wins,
//!    if it is better than the current one by more than the margin of the pack (a lower margin
//!    when the winner is a dictionary word).
//!
//! The leading and trailing keys that are punctuation in some layouts and letters in others (`,` is
//! `б` and `'` is `э` in Russian) are tried both as part of the word and as punctuation around it.

use std::sync::Arc;

use lite_switcher_core::word::KeyStroke;

use crate::pack::{id_matches, Loaded, LoadedPack};
use crate::render::{KeyClass, KeyRenderer, RenderTable};

/// How much cheaper (cost units, 1/8 bit) a candidate in the language of the previous word is.
pub const CONTEXT_BONUS: u32 = 12;
/// The most keys at each end of the word that are tried as punctuation.
const MAX_STRIP: usize = 2;
/// The cost of a word that is impossible in the current layout (per trigram).
const IMPOSSIBLE_PER_TRIGRAM: u32 = 255;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeepReason {
    /// The current layout has no active literary pack.
    NoPack,
    TooShort,
    /// An abbreviation in capitals.
    AllCaps,
    /// The word is in `exceptions.txt`.
    Exception,
    /// The word is in the dictionary of the current layout.
    Dictionary,
    /// No other layout gives a word.
    NoCandidate,
    /// The best candidate is not better enough than the current layout.
    BelowMargin,
    /// The word would look the same in the better layout (ru/uk): nothing to fix in the text.
    SameText,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Replace {
    /// Index of the layout in the table.
    pub layout: usize,
    /// The whole word in that layout (including the trailing punctuation).
    pub text: String,
    /// How many keys around the word (leading and trailing) are punctuation, not letters.
    pub stripped: usize,
    /// How much better than the current layout (cost units).
    pub margin: u32,
    /// The winner is a dictionary word.
    pub dictionary: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    Keep(KeepReason),
    Replace(Replace),
}

/// What the detector knows about the word.
pub struct Context<'a> {
    pub keys: &'a [KeyStroke],
    /// The layout that was active when the word was typed (index in the table).
    pub current: usize,
    /// The layout of the previous word (index in the table), if it is recent.
    pub previous: Option<usize>,
}

pub struct LiteraryDetector {
    /// For every layout of the table: the pack that serves it.
    layout_pack: Vec<Option<usize>>,
    packs: Vec<Arc<LoadedPack>>,
}

struct Evaluated {
    in_dictionary: bool,
    in_exceptions: bool,
    all_caps: bool,
    /// Cost of the whole word scaled to the length of the typed keys; `None` if it is not a word.
    cost: Option<u32>,
    word_len: usize,
}

fn fold(word: String, fold_yo: bool) -> String {
    if fold_yo { word.replace('ё', "е") } else { word }
}

impl LiteraryDetector {
    /// `table_ids` are the ids of the layouts of the render table, in its order.
    pub fn new(loaded: &Loaded, table_ids: &[String]) -> LiteraryDetector {
        let layout_pack = table_ids
            .iter()
            .map(|id| {
                loaded.literary.iter().position(|p| p.manifest.layout_match.iter().any(|m| id_matches(id, m)))
            })
            .collect();
        LiteraryDetector { layout_pack, packs: loaded.literary.clone() }
    }

    /// Is there a pack for this layout of the table?
    pub fn serves(&self, layout: usize) -> bool {
        self.layout_pack.get(layout).copied().flatten().is_some()
    }

    /// Reads the keys as `lead` punctuation keys, a word and `trail` punctuation keys in `layout`.
    fn evaluate(&self, table: &RenderTable, keys: &[KeyStroke], layout: usize, lead: usize, trail: usize) -> Option<Evaluated> {
        let pack = &self.packs[self.layout_pack.get(layout).copied().flatten()?];
        let n = keys.len();
        let end = n.checked_sub(trail)?;
        if lead >= end {
            return None;
        }
        let word_keys = &keys[lead..end];
        // the stripped keys have to be punctuation in this layout
        for key in keys[..lead].iter().chain(&keys[end..]) {
            if table.render(key, layout).is_none_or(char::is_alphabetic) {
                return None;
            }
        }
        let typed = table.render_word(word_keys, layout)?;
        let params = &pack.manifest.literary;
        let lower = fold(typed.to_lowercase(), params.fold_yo);
        let letters: Vec<char> = typed.chars().filter(|c| c.is_alphabetic()).collect();
        let all_caps = letters.len() >= 2 && letters.iter().all(|c| c.is_uppercase());
        let word_len = word_keys.len();
        let is_word = letters.len() == typed.chars().count()
            && !params.forbidden_bigrams.iter().any(|b| lower.contains(b.as_str()));
        let cost = if is_word {
            pack.ngram.as_ref().and_then(|ng| ng.cost(&lower)).map(|c| (c * (n as u32 + 1)).div_ceil(word_len as u32 + 1))
        } else {
            None
        };
        Some(Evaluated {
            in_dictionary: pack.words.contains(&lower),
            in_exceptions: pack.exceptions.contains(&lower),
            all_caps,
            cost,
            word_len,
        })
    }

    pub fn decide(&self, table: &RenderTable, ctx: &Context) -> Verdict {
        let keys = ctx.keys;
        let n = keys.len();
        let Some(current_pack) = self.layout_pack.get(ctx.current).copied().flatten() else {
            return Verdict::Keep(KeepReason::NoPack);
        };
        if n < self.packs[current_pack].manifest.literary.min_word_len {
            return Verdict::Keep(KeepReason::TooShort);
        }
        let ambiguous = |k: &&KeyStroke| table.class(k) == KeyClass::Ambiguous;
        let lead_max = keys.iter().take_while(ambiguous).count().min(MAX_STRIP);
        let trail_max = keys.iter().rev().take_while(ambiguous).count().min(MAX_STRIP);
        let bonus = |layout: usize| if ctx.previous == Some(layout) { CONTEXT_BONUS } else { 0 };

        // 1. the word in the current layout
        let mut current_cost: Option<u32> = None;
        for (lead, trail) in (0..=lead_max).flat_map(|l| (0..=trail_max).map(move |t| (l, t))) {
            let Some(e) = self.evaluate(table, keys, ctx.current, lead, trail) else { continue };
            if e.all_caps {
                return Verdict::Keep(KeepReason::AllCaps);
            }
            if e.in_exceptions {
                return Verdict::Keep(KeepReason::Exception);
            }
            if e.in_dictionary {
                return Verdict::Keep(KeepReason::Dictionary);
            }
            if let Some(c) = e.cost {
                current_cost = Some(current_cost.map_or(c, |m| m.min(c)));
            }
        }
        let current_total = current_cost
            .map_or(IMPOSSIBLE_PER_TRIGRAM * (n as u32 + 1), |c| c.saturating_sub(bonus(ctx.current)));

        // 2. the best word in the other layouts
        struct Best {
            layout: usize,
            strip: usize,
            cost: u32,
            dictionary: bool,
        }
        // a dictionary word beats a word that only looks like one; then the cheaper wins
        let better = |new: &Best, old: &Best| (!new.dictionary, new.cost) < (!old.dictionary, old.cost);
        let mut best: Option<Best> = None;
        for layout in 0..table.layout_count() {
            let Some(pack_index) = self.layout_pack.get(layout).copied().flatten() else { continue };
            if layout == ctx.current {
                continue;
            }
            let min_len = self.packs[pack_index].manifest.literary.min_word_len;
            for (lead, trail) in (0..=lead_max).flat_map(|l| (0..=trail_max).map(move |t| (l, t))) {
                let Some(e) = self.evaluate(table, keys, layout, lead, trail) else { continue };
                if e.word_len < min_len.min(n) {
                    continue;
                }
                let Some(cost) = e.cost else { continue };
                let cost = cost.saturating_sub(bonus(layout));
                let candidate = Best { layout, strip: lead + trail, cost, dictionary: e.in_dictionary || e.in_exceptions };
                if best.as_ref().is_none_or(|b| better(&candidate, b)) {
                    best = Some(candidate);
                }
            }
        }
        let Some(best) = best else { return Verdict::Keep(KeepReason::NoCandidate) };
        let params = &self.packs[self.layout_pack[best.layout].expect("checked above")].manifest.literary;
        if best.cost > params.max_average_cost * (n as u32 + 1) {
            return Verdict::Keep(KeepReason::NoCandidate);
        }
        let Some(margin) = current_total.checked_sub(best.cost).filter(|m| *m > 0) else {
            return Verdict::Keep(KeepReason::BelowMargin);
        };
        let needed = if best.dictionary { params.dict_margin } else { params.switch_margin };
        if margin < needed {
            return Verdict::Keep(KeepReason::BelowMargin);
        }
        // Replacing a word with the same text would only flip the layout (ru and uk share most
        // letters); the text is right already, and a wrong flip is annoying.
        if table.render_word(keys, ctx.current) == table.render_word(keys, best.layout) {
            return Verdict::Keep(KeepReason::SameText);
        }
        match table.render_word(keys, best.layout) {
            Some(text) => Verdict::Replace(Replace {
                layout: best.layout,
                text,
                stripped: best.strip,
                margin,
                dictionary: best.dictionary,
            }),
            None => Verdict::Keep(KeepReason::NoCandidate),
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::build::{frequency_class, train_ngram, training_weight};
    use crate::format::Words;
    use crate::manifest::{tests::LITERARY, Manifest};
    use crate::render::presets::{keys_for, table};
    use std::collections::HashSet;

    pub const RU_WORDS: &[&str] = &[
        "привет", "пока", "дом", "мама", "папа", "хорошо", "работа", "время", "человек", "день", "рука", "земля",
        "голова", "вода", "город", "жизнь", "слово", "дело", "сказать", "который", "большой", "новый", "первый",
        "свой", "друг", "вопрос", "после", "всегда", "может", "теперь", "сейчас", "здесь", "очень", "тоже",
        "спасибо", "пожалуйста", "хотеть", "знать", "видеть", "делать", "говорить", "понимать", "работать",
        "ребенок", "женщина", "мужчина", "школа", "книга", "письмо", "ночь", "утро", "вечер", "зима", "лето",
    ];
    pub const UK_WORDS: &[&str] = &[
        "привіт", "дякую", "будь", "ласка", "добре", "робота", "година", "людина", "день", "рука", "земля",
        "голова", "вода", "місто", "життя", "слово", "справа", "сказати", "який", "великий", "новий", "перший",
        "свій", "друг", "питання", "після", "завжди", "може", "тепер", "зараз", "тут", "дуже", "теж", "хотіти",
        "знати", "бачити", "робити", "говорити", "розуміти", "працювати", "дитина", "жінка", "чоловік",
        "школа", "книга", "лист", "ніч", "ранок", "вечір", "зима", "літо", "мати", "батько",
    ];
    pub const EN_WORDS: &[&str] = &[
        "hello", "world", "the", "and", "that", "have", "for", "not", "with", "you", "this", "but", "his", "from",
        "they", "say", "her", "she", "will", "one", "all", "would", "there", "their", "what", "out", "about",
        "who", "get", "which", "when", "make", "can", "like", "time", "just", "him", "know", "take", "people",
        "into", "year", "your", "good", "some", "could", "them", "see", "other", "than", "then", "now", "look",
        "only", "come", "its", "over", "think", "also", "back", "after", "use", "two", "how", "work", "first",
        "well", "way", "even", "new", "want", "because", "any", "these", "give", "day", "most", "us", "don't",
    ];

    fn alphabet(words: &[&str]) -> Vec<char> {
        let mut set: Vec<char> = words.iter().flat_map(|w| w.chars()).filter(|c| *c != '\'').collect::<HashSet<_>>().into_iter().collect();
        set.sort();
        set
    }

    pub(crate) fn pack(code: &str, words: &[&str], exceptions: &[&str]) -> Arc<LoadedPack> {
        let text = LITERARY
            .replace("code = ru", &format!("code = {code}"))
            .replace("match = ru", &format!("match = {code}"))
            .replace("min_word_len = 4", "min_word_len = 3")
            .replace("switch_margin = 12", "switch_margin = 100")
            .replace("dict_margin = 3", "dict_margin = 20")
            .replace("fold_yo = true", "fold_yo = false")
            .replace("forbidden_bigrams = ъы, ьь", "forbidden_bigrams = ъы");
        let manifest = Manifest::parse(&text, Some(code)).unwrap();
        let ngram = train_ngram(&alphabet(words), words.iter().map(|w| (*w, training_weight(100))));
        let bytes = Words::build(words.iter().map(|w| (w.to_string(), frequency_class(100)))).unwrap();
        Arc::new(LoadedPack {
            manifest,
            words: Words::from_bytes(bytes).unwrap(),
            ngram: Some(ngram),
            exceptions: exceptions.iter().map(|e| e.to_string()).collect(),
        })
    }

    fn detector() -> (LiteraryDetector, RenderTable) {
        let loaded = Loaded {
            literary: vec![pack("en", EN_WORDS, &[]), pack("ru", RU_WORDS, &["нато"]), pack("uk", UK_WORDS, &[])],
            programming: None,
        };
        let table = table(&["en", "ru", "uk"]);
        (LiteraryDetector::new(&loaded, table.layout_ids()), table)
    }

    const EN: usize = 0;
    const RU: usize = 1;
    const UK: usize = 2;

    fn decide(d: &LiteraryDetector, t: &RenderTable, text: &str, typed_in: &str, current: usize) -> Verdict {
        let keys = keys_for(text, typed_in);
        d.decide(t, &Context { keys: &keys, current, previous: None })
    }

    #[test]
    fn restores_a_word_typed_in_the_wrong_layout() {
        let (d, t) = detector();
        match decide(&d, &t, "привет", "ru", EN) {
            Verdict::Replace(r) => assert_eq!((r.layout, r.text.as_str(), r.stripped), (RU, "привет", 0)),
            other => panic!("{other:?}"),
        }
        match decide(&d, &t, "hello", "en", RU) {
            Verdict::Replace(r) => assert_eq!((r.layout, r.text.as_str()), (EN, "hello")),
            other => panic!("{other:?}"),
        }
        match decide(&d, &t, "привіт", "uk", EN) {
            Verdict::Replace(r) => assert_eq!((r.layout, r.text.as_str()), (UK, "привіт")),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn keeps_words_that_are_fine_in_the_current_layout() {
        let (d, t) = detector();
        assert_eq!(decide(&d, &t, "привет", "ru", RU), Verdict::Keep(KeepReason::Dictionary));
        assert_eq!(decide(&d, &t, "hello", "en", EN), Verdict::Keep(KeepReason::Dictionary));
        assert_eq!(decide(&d, &t, "привіт", "uk", UK), Verdict::Keep(KeepReason::Dictionary));
    }

    #[test]
    fn a_word_with_a_comma_or_period_typed_correctly_is_kept() {
        let (d, t) = detector();
        // `,` is the key of `б` in Russian: "hello," is "hello" and a comma
        assert_eq!(decide(&d, &t, "hello,", "en", EN), Verdict::Keep(KeepReason::Dictionary));
        assert_eq!(decide(&d, &t, "hello.", "en", EN), Verdict::Keep(KeepReason::Dictionary));
        // the apostrophe of an English contraction is in the dictionary
        assert_eq!(decide(&d, &t, "don't", "en", EN), Verdict::Keep(KeepReason::Dictionary));
    }

    #[test]
    fn a_word_in_quotes_typed_correctly_is_kept() {
        let (d, t) = detector();
        // `'` is the key of `э` in Russian and `"` is Shift+`э`: 'hello' and "hello"
        assert_eq!(decide(&d, &t, "'hello'", "en", EN), Verdict::Keep(KeepReason::Dictionary));
        assert_eq!(decide(&d, &t, "\"hello\"", "en", EN), Verdict::Keep(KeepReason::Dictionary));
        assert_eq!(decide(&d, &t, "'hello", "en", EN), Verdict::Keep(KeepReason::Dictionary));
    }

    #[test]
    fn a_dictionary_word_beats_a_word_that_only_looks_like_one() {
        // "який" is a Ukrainian word, "привет" a Russian one: typed in en they are found in their own language
        let (d, t) = detector();
        let pick = |word: &str, lang: &str| match decide(&d, &t, word, lang, EN) {
            Verdict::Replace(r) => r.layout,
            other => panic!("{word}: {other:?}"),
        };
        assert_eq!(pick("який", "uk"), UK);
        assert_eq!(pick("привет", "ru"), RU);
        assert_eq!(pick("привіт", "uk"), UK);
    }

    #[test]
    fn a_word_that_looks_the_same_in_ru_and_uk_is_not_touched() {
        let (d, t) = detector();
        // "рука" typed in the Ukrainian layout looks exactly the same: nothing to fix
        assert_eq!(decide(&d, &t, "голова", "ru", UK), Verdict::Keep(KeepReason::Dictionary));
        let loaded = Loaded { literary: vec![pack("ru", &["пьесы", "собака", "ночные"], &[]), pack("uk", UK_WORDS, &[])], programming: None };
        let t = table(&["ru", "uk"]);
        let d = LiteraryDetector::new(&loaded, t.layout_ids());
        let keys = keys_for("собака", "ru");
        // typed in uk the text is the same: kept although the Russian model likes it better
        assert!(matches!(
            d.decide(&t, &Context { keys: &keys, current: 1, previous: None }),
            Verdict::Keep(KeepReason::SameText | KeepReason::BelowMargin | KeepReason::NoCandidate)
        ));
    }

    #[test]
    fn punctuation_keys_after_a_wrong_layout_word_are_restored_too() {
        let (d, t) = detector();
        // "hello," typed in ru: the key of `,` is `б` there
        match decide(&d, &t, "руддщб", "ru", RU) {
            Verdict::Replace(r) => assert_eq!((r.layout, r.text.as_str(), r.stripped), (EN, "hello,", 1)),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn letters_on_punctuation_keys_make_the_english_reading_impossible() {
        let (d, t) = detector();
        // "хорошо" typed in en is "[jhj im": the `[` key is not a letter in en
        match decide(&d, &t, "хорошо", "ru", EN) {
            Verdict::Replace(r) => assert_eq!((r.layout, r.text.as_str()), (RU, "хорошо")),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn numbers_abbreviations_exceptions_and_short_words_are_left_alone() {
        let (d, t) = detector();
        let with_digits = keys_for("ghbdtn1", "en");
        assert_eq!(
            d.decide(&t, &Context { keys: &with_digits, current: EN, previous: None }),
            Verdict::Keep(KeepReason::NoCandidate)
        );
        let caps = keys_for("NATO", "en");
        assert_eq!(d.decide(&t, &Context { keys: &caps, current: EN, previous: None }), Verdict::Keep(KeepReason::AllCaps));
        assert_eq!(decide(&d, &t, "нато", "ru", RU), Verdict::Keep(KeepReason::Exception));
        assert_eq!(decide(&d, &t, "до", "ru", EN), Verdict::Keep(KeepReason::TooShort));
        let none: Vec<KeyStroke> = Vec::new();
        assert!(matches!(d.decide(&t, &Context { keys: &none, current: EN, previous: None }), Verdict::Keep(_)));
    }

    #[test]
    fn no_pack_for_the_current_layout_means_no_decision() {
        let loaded = Loaded { literary: vec![pack("ru", RU_WORDS, &[])], programming: None };
        let t = table(&["en", "ru"]);
        let d = LiteraryDetector::new(&loaded, t.layout_ids());
        assert!(d.serves(1) && !d.serves(0));
        let keys = keys_for("привет", "ru");
        assert_eq!(d.decide(&t, &Context { keys: &keys, current: 0, previous: None }), Verdict::Keep(KeepReason::NoPack));
    }

    #[test]
    fn the_previous_word_breaks_a_tie_between_ru_and_uk() {
        // two packs trained on the same words cost the same: the language of the previous word wins
        let loaded = Loaded {
            literary: vec![pack("en", EN_WORDS, &[]), pack("ru", RU_WORDS, &[]), pack("uk", RU_WORDS, &[])],
            programming: None,
        };
        let t = table(&["en", "ru", "uk"]);
        let d = LiteraryDetector::new(&loaded, t.layout_ids());
        let keys = keys_for("привет", "ru");
        let pick = |previous: Option<usize>| match d.decide(&t, &Context { keys: &keys, current: EN, previous }) {
            Verdict::Replace(r) => r.layout,
            other => panic!("{other:?}"),
        };
        assert_eq!(pick(Some(UK)), UK);
        assert_eq!(pick(Some(RU)), RU);
    }

    #[test]
    fn the_margin_protects_against_weak_evidence() {
        // a strict pack (huge margin) never replaces a word that is not a dictionary word
        let strict = |code: &str, words: &[&str]| {
            let mut p = pack(code, words, &[]);
            let m = Arc::get_mut(&mut p).unwrap();
            m.manifest.literary.switch_margin = 100_000;
            m.words = Words::from_bytes(Words::build([("zzzz".to_string(), 1)]).unwrap()).unwrap();
            p
        };
        let loaded = Loaded { literary: vec![pack("en", EN_WORDS, &[]), strict("ru", RU_WORDS)], programming: None };
        let t = table(&["en", "ru"]);
        let d = LiteraryDetector::new(&loaded, t.layout_ids());
        let keys = keys_for("привет", "ru");
        assert_eq!(d.decide(&t, &Context { keys: &keys, current: 0, previous: None }), Verdict::Keep(KeepReason::BelowMargin));
    }
}
