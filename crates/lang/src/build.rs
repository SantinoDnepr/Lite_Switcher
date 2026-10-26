//! Building the data of a pack (used by `dictgen` and by the tests).

use std::collections::HashMap;

use crate::format::{Ngram, NGRAM_IMPOSSIBLE};

/// Units of the trigram cost: 1/8 of a bit (`-log2 p * 8`).
pub const COST_UNITS_PER_BIT: f64 = 8.0;

/// Smoothing of the trigram probabilities.
const SMOOTHING: f64 = 0.02;

/// The weight of a word with `count` occurrences in the training: frequent words count more,
/// but not linearly.
pub fn training_weight(count: u64) -> f64 {
    1.0 + (count.max(1) as f64).ln()
}

/// The frequency class stored in `words.fst` (bits 0-15): logarithmic, 0 = unknown.
pub fn frequency_class(count: u64) -> u32 {
    ((count as f64 + 1.0).log2() * 256.0).round().min(65535.0) as u32
}

/// Trains the trigram table of a language. `words` are lower-case words made of the letters of the
/// alphabet (other words are ignored) with their weights.
pub fn train_ngram<'a>(alphabet: &[char], words: impl IntoIterator<Item = (&'a str, f64)>) -> Ngram {
    let side = alphabet.len() + 1;
    let index: HashMap<char, usize> = alphabet.iter().enumerate().map(|(i, c)| (*c, i + 1)).collect();
    let mut tri = vec![0f64; side * side * side];
    let mut bi = vec![0f64; side * side];
    for (word, weight) in words {
        let mut idx = vec![0usize, 0];
        let mut known = true;
        for c in word.chars() {
            match index.get(&c) {
                Some(i) => idx.push(*i),
                None => {
                    known = false;
                    break;
                }
            }
        }
        if !known || idx.len() == 2 {
            continue;
        }
        idx.push(0);
        for w in idx.windows(3) {
            tri[(w[0] * side + w[1]) * side + w[2]] += weight;
            bi[w[0] * side + w[1]] += weight;
        }
    }
    let mut table = vec![0u8; side * side * side];
    for a in 0..side {
        for b in 0..side {
            for c in 0..side {
                let p = (tri[(a * side + b) * side + c] + SMOOTHING) / (bi[a * side + b] + SMOOTHING * side as f64);
                let cost = (-p.log2() * COST_UNITS_PER_BIT).round().clamp(0.0, f64::from(NGRAM_IMPOSSIBLE - 1));
                table[(a * side + b) * side + c] = cost as u8;
            }
        }
    }
    Ngram::new(alphabet.to_vec(), table).expect("the table size matches the alphabet")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frequency_class_grows_slowly() {
        assert_eq!(frequency_class(0), 0);
        assert!(frequency_class(10) < frequency_class(1000));
        assert!(frequency_class(u64::MAX) <= 65535);
    }

    #[test]
    fn trained_costs_prefer_the_trained_language() {
        let alphabet: Vec<char> = "abcdefghijklmnopqrstuvwxyz".chars().collect();
        let words = ["the", "then", "there", "this", "that", "they", "hello", "world", "word", "work"];
        let ngram = train_ngram(&alphabet, words.iter().map(|w| (*w, 1.0)));
        let english = ngram.cost("them").unwrap();
        let noise = ngram.cost("qzxv").unwrap();
        assert!(english < noise, "{english} < {noise}");
        assert_eq!(ngram.cost("тест"), None, "a letter outside the alphabet");
        assert_eq!(Ngram::parse(&ngram.to_bytes()).unwrap(), ngram);
    }
}
