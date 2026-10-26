//! `dictgen literary`: a word-count list -> `words.fst`, `ngram3.bin`, `pack.ini`.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use lite_switcher_lang::build::{frequency_class, train_ngram, training_weight};
use lite_switcher_lang::format::{parse_exceptions, sha256_hex, Words};

use crate::Args;

/// The alphabet of a language when `--alphabet` is not given.
fn preset_alphabet(code: &str) -> Option<&'static str> {
    match code {
        "en" => Some("abcdefghijklmnopqrstuvwxyz"),
        "ru" => Some("абвгдеёжзийклмнопрстуфхцчшщъыьэюя"),
        "uk" => Some("абвгґдеєжзиіїйклмнопрстуфхцчшщьюя"),
        _ => None,
    }
}

/// Reads `word count` lines; the words are lower-cased, the counts of equal words are added up.
/// Only words made of `alphabet` letters (and `extra` characters such as an apostrophe) are kept.
pub fn read_counts(path: &Path, alphabet: &[char], extra: &[char]) -> Result<Vec<(String, u64)>, String> {
    let text = fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut counts: HashMap<String, u64> = HashMap::new();
    for line in text.lines() {
        let mut parts = line.split_whitespace();
        let (Some(word), Some(count)) = (parts.next(), parts.next()) else { continue };
        let Ok(count) = count.parse::<u64>() else { continue };
        let word = word.to_lowercase();
        if word.chars().all(|c| alphabet.contains(&c) || extra.contains(&c)) && word.chars().any(|c| alphabet.contains(&c)) {
            *counts.entry(word).or_default() += count;
        }
    }
    let mut words: Vec<(String, u64)> = counts.into_iter().collect();
    words.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    Ok(words)
}

pub fn run(args: &Args) -> Result<(), String> {
    let code = args.required("code")?;
    let name = args.required("name")?;
    let out = PathBuf::from(args.required("out")?);
    let freq = PathBuf::from(args.required("freq")?);
    let license = args.required("license")?;
    let sources = args.required("sources")?;
    let alphabet_text = match args.get("alphabet").filter(|a| !a.is_empty()) {
        Some(a) => a.to_string(),
        None => preset_alphabet(code).ok_or("--alphabet is required for this language")?.to_string(),
    };
    let alphabet: Vec<char> = alphabet_text.chars().collect();
    let layout_match = args.get("match").filter(|m| !m.is_empty()).unwrap_or(code);
    let min_count: u64 = args.number("min-count", 30)?;
    let max_words: usize = args.number("max-words", usize::MAX)?;
    let min_len: usize = args.number("min-len", 3)?;
    let switch_margin: u32 = args.number("switch-margin", 180)?;
    let dict_margin: u32 = args.number("dict-margin", 23)?;
    let max_cost: u32 = args.number("max-cost", 48)?;
    let fold_yo = args.flag("fold-yo");
    let forbidden = args.get("forbidden").unwrap_or_default();
    let unique = args.get("unique").unwrap_or_default();

    // the apostrophe is not a letter but English words contain it ("don't"): such words are only
    // looked up in the dictionary, they do not train the trigrams
    let extra: Vec<char> = if code == "en" { vec!['\''] } else { Vec::new() };
    let all = read_counts(&freq, &alphabet, &extra)?;
    let fold = |w: &str| if fold_yo { w.replace('ё', "е") } else { w.to_string() };
    let dictionary: Vec<&(String, u64)> = all.iter().filter(|(_, c)| *c >= min_count).take(max_words).collect();

    // forbidden bigrams must not occur in the dictionary (they would make real words impossible)
    for bigram in forbidden.split(',').map(str::trim).filter(|b| !b.is_empty()) {
        let hits: Vec<&str> = dictionary.iter().filter(|(w, _)| w.contains(bigram)).map(|(w, _)| w.as_str()).take(3).collect();
        if !hits.is_empty() {
            return Err(format!("the forbidden bigram '{bigram}' occurs in the dictionary: {hits:?}"));
        }
    }

    fs::create_dir_all(&out).map_err(|e| format!("{}: {e}", out.display()))?;
    let words_bytes = Words::build(dictionary.iter().map(|(w, c)| (fold(w), frequency_class(*c)))).map_err(|e| e.to_string())?;
    let training: Vec<(String, f64)> = dictionary
        .iter()
        .filter(|(w, _)| w.chars().all(|c| alphabet.contains(&c)))
        .map(|(w, c)| (fold(w), training_weight(*c)))
        .collect();
    let ngram_bytes = train_ngram(&alphabet, training.iter().map(|(w, weight)| (w.as_str(), *weight))).to_bytes();
    crate::check_size("words.fst", &words_bytes)?;
    crate::check_size("ngram3.bin", &ngram_bytes)?;
    fs::write(out.join("words.fst"), &words_bytes).map_err(|e| e.to_string())?;
    fs::write(out.join("ngram3.bin"), &ngram_bytes).map_err(|e| e.to_string())?;

    let mut files = String::from("words = words.fst\nngram = ngram3.bin\n");
    let mut checks = format!("sha256_words = {}\nsha256_ngram = {}\n", sha256_hex(&words_bytes), sha256_hex(&ngram_bytes));
    if let Some(path) = args.get("exceptions").filter(|p| !p.is_empty()) {
        let text = fs::read_to_string(path).map_err(|e| format!("{path}: {e}"))?;
        let mut set: Vec<String> = parse_exceptions(&text).into_iter().collect();
        set.sort();
        let body = set.join("\n") + "\n";
        fs::write(out.join("exceptions.txt"), &body).map_err(|e| e.to_string())?;
        files.push_str("exceptions = exceptions.txt\n");
        checks.push_str(&format!("sha256_exceptions = {}\n", sha256_hex(body.as_bytes())));
    }
    let manifest = format!(
        "[pack]\nkind = literary\ncode = {code}\nname = {name}\nformat = 1\nversion = 1\nlicense = {license}\nsources = {sources}\n\n\
         [layouts]\nmatch = {layout_match}\n\n[files]\n{files}{checks}\n\
         [literary]\nalphabet = {alphabet_text}\nunique_letters = {unique}\nforbidden_bigrams = {forbidden}\nmin_word_len = {min_len}\n\
         switch_margin = {switch_margin}\ndict_margin = {dict_margin}\nmax_average_cost = {max_cost}\nfold_yo = {fold_yo}\n"
    );
    fs::write(out.join("pack.ini"), manifest).map_err(|e| e.to_string())?;

    println!(
        "{code}: {} words in the dictionary (count >= {min_count}) of {} read; words.fst {} bytes, ngram3.bin {} bytes",
        dictionary.len(),
        all.len(),
        words_bytes.len(),
        ngram_bytes.len()
    );
    if dictionary.len() < 30_000 {
        println!("warning: the dictionary has fewer than 30000 words (specification FR-L9)");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Args;
    use lite_switcher_lang::manifest::Kind;
    use lite_switcher_lang::pack::{discover_dir, load_pack};

    fn args(list: &[&str]) -> Args {
        Args::parse(&list.iter().map(|s| s.to_string()).collect::<Vec<_>>()).unwrap()
    }

    #[test]
    fn reads_counts_and_filters_by_the_alphabet() {
        let dir = std::env::temp_dir().join(format!("ls-dictgen-read-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("list.txt");
        fs::write(&path, "the 100\nThe 50\ndon't 40\n123 9\nпривет 8\nbad 1x\nsingle\nzz 3\n").unwrap();
        let alphabet: Vec<char> = "abcdefghijklmnopqrstuvwxyz".chars().collect();
        let words = read_counts(&path, &alphabet, &['\'']).unwrap();
        assert_eq!(words, vec![("the".to_string(), 150), ("don't".to_string(), 40), ("zz".to_string(), 3)]);
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn builds_a_pack_that_the_loader_accepts() {
        let dir = std::env::temp_dir().join(format!("ls-dictgen-pack-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let list = dir.join("en.txt");
        let text: String = ["the 500", "hello 300", "world 200", "work 150", "rare 2", "don't 90"]
            .iter()
            .map(|l| format!("{l}\n"))
            .collect();
        fs::write(&list, text).unwrap();
        let root = dir.join("LiteraryLanguages");
        let out = root.join("en");
        run(&args(&[
            "--code", "en", "--name", "English", "--freq", list.to_str().unwrap(), "--out", out.to_str().unwrap(),
            "--license", "CC-BY-SA-4.0", "--sources", "test", "--min-count", "50",
        ]))
        .unwrap();
        let (packs, issues) = discover_dir(&root, Kind::Literary);
        assert!(issues.is_empty(), "{issues:?}");
        let pack = load_pack(&packs[0]).unwrap();
        assert_eq!(pack.words.len(), 5, "the rare word is not in the dictionary");
        assert!(pack.words.contains("don't") && !pack.words.contains("rare"));
        let ngram = pack.ngram.as_ref().unwrap();
        assert!(ngram.cost("hello").is_some());
        assert_eq!(ngram.cost("don't"), None, "words with an apostrophe do not train the trigrams");
        // a forbidden bigram that occurs in the dictionary is refused
        let bad = args(&[
            "--code", "en", "--name", "English", "--freq", list.to_str().unwrap(), "--out", out.to_str().unwrap(),
            "--license", "x", "--sources", "x", "--min-count", "50", "--forbidden", "wo",
        ]);
        assert!(run(&bad).unwrap_err().contains("forbidden bigram"));
        fs::remove_dir_all(&dir).unwrap();
    }
}
