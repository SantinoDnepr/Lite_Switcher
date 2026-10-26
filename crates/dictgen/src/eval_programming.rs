//! `dictgen eval-programming`: does the programming detector find the words of the language, and
//! does it leave real code alone?
//!
//! 1. Every form of the seed is typed in 4 ways (as it is, in capitals, with a capital first letter,
//!    and in the wrong layout): the right layout must be kept, the wrong one restored.
//! 2. Tokens of real code (a weighted sample of the occurrences in the `.bsl` files) are typed
//!    correctly: the detector must not change them.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Instant;

use lite_switcher_core::word::KeyStroke;
use lite_switcher_lang::literary::Context;
use lite_switcher_lang::manifest::Kind;
use lite_switcher_lang::pack::{discover_dir, load_pack};
use lite_switcher_lang::programming::{ProgrammingDetector, Verdict};
use lite_switcher_lang::render::presets::{table, try_keys_for};
use lite_switcher_lang::render::RenderTable;

use crate::programming::{parse_seed, tokens};
use crate::Args;

const EN: usize = 0;
const RU: usize = 1;

fn is_cyrillic(c: char) -> bool {
    ('\u{0400}'..='\u{04FF}').contains(&c)
}

/// The keys that type `text`: Cyrillic letters with the Russian keys, everything else (Latin
/// letters, digits, `_`, the sigils `#` and `&`) with the English ones. The layout of the letters
/// (`None` if there are letters of both kinds).
fn type_text(text: &str) -> Option<(Vec<KeyStroke>, Option<usize>)> {
    let mut keys = Vec::new();
    let (mut cyrillic, mut latin) = (false, false);
    for c in text.chars() {
        let layout = if is_cyrillic(c) {
            cyrillic = true;
            "ru"
        } else {
            latin |= c.is_ascii_alphabetic();
            "en"
        };
        keys.extend(try_keys_for(&c.to_string(), layout)?);
    }
    let layout = match (cyrillic, latin) {
        (true, false) => Some(RU),
        (false, true) => Some(EN),
        (false, false) => Some(EN),
        (true, true) => None,
    };
    Some((keys, layout))
}

fn variants(form: &str) -> Vec<String> {
    let mut title = String::new();
    let mut after_sigil = true;
    for c in form.chars() {
        if after_sigil && (c == '#' || c == '&') {
            title.push(c);
            continue;
        }
        if after_sigil {
            title.extend(c.to_uppercase());
            after_sigil = false;
        } else {
            title.push(c);
        }
    }
    vec![form.to_string(), form.to_uppercase(), title]
}

struct Env<'a> {
    detector: &'a ProgrammingDetector,
    table: &'a RenderTable,
}

impl Env<'_> {
    fn decide(&self, keys: &[KeyStroke], current: usize) -> Verdict {
        self.detector.decide(self.table, &Context { keys, current, previous: None })
    }
}

fn eval_seed(env: &Env, seed_path: &str) -> Result<bool, String> {
    let text = fs::read_to_string(seed_path).map_err(|e| format!("{seed_path}: {e}"))?;
    // the forms of the seed (the parser lower-cases them: the case variants are made here)
    let forms: Vec<String> = parse_seed(&text)?.into_iter().map(|(w, _)| w).filter(|w| w.chars().count() >= 3).collect();
    let (mut total, mut ok) = (0u32, 0u32);
    let mut failures: Vec<String> = Vec::new();
    for form in &forms {
        for variant in variants(form) {
            let Some((keys, layout)) = type_text(&variant) else { continue };
            let current_right = layout.unwrap_or(RU);
            let current_wrong = if current_right == RU { EN } else { RU };
            // typed in the right layout: kept, or "replaced" with the very same text
            let right = match env.decide(&keys, current_right) {
                Verdict::Keep(_) => true,
                Verdict::Replace(r) => r.text == variant,
            };
            // typed in the wrong layout: the word is restored; a word of one language also gets its layout
            let wrong = match env.decide(&keys, current_wrong) {
                Verdict::Replace(r) => r.text == variant && (layout.is_none() || r.layout == layout || r.layout.is_none()),
                Verdict::Keep(_) => false,
            };
            total += 2;
            ok += u32::from(right) + u32::from(wrong);
            if (!right || !wrong) && failures.len() < 12 {
                failures.push(format!("{variant}: right layout {}, wrong layout {}", if right { "ok" } else { "FAILED" }, if wrong { "ok" } else { "FAILED" }));
            }
        }
    }
    println!("\n== seed: {} forms of 3 or more keys, 3 case variants, 2 layouts each ==", forms.len());
    println!("correct decisions: {ok} of {total} ({:.2}%)", f64::from(ok) * 100.0 / f64::from(total.max(1)));
    for failure in &failures {
        println!("  {failure}");
    }
    Ok(ok == total)
}

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
}

fn corpus_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            corpus_files(&path, out);
        } else if path.extension().is_some_and(|e| e.eq_ignore_ascii_case("bsl")) {
            out.push(path);
        }
    }
}

fn eval_replay(env: &Env, root: &str, sample: usize) -> Result<bool, String> {
    let mut files = Vec::new();
    corpus_files(Path::new(root), &mut files);
    // the tokens of the unique files, with their occurrences
    let mut seen = std::collections::HashSet::new();
    let mut counts: HashMap<String, u64> = HashMap::new();
    for path in &files {
        let Ok(bytes) = fs::read(path) else { continue };
        if !seen.insert(lite_switcher_lang::format::sha256_hex(&bytes)) {
            continue;
        }
        tokens(String::from_utf8_lossy(&bytes).trim_start_matches('\u{feff}'), |t| *counts.entry(t).or_default() += 1);
    }
    // only tokens that can be typed with one layout (hybrids are typed with a change of layout)
    let mut typable: Vec<(String, u64, Vec<KeyStroke>, usize)> = counts
        .into_iter()
        .filter(|(t, _)| t.chars().count() >= 3)
        .filter_map(|(t, n)| {
            let (keys, layout) = type_text(&t)?;
            Some((t, n, keys, layout?))
        })
        .collect();
    typable.sort_by(|a, b| a.0.cmp(&b.0));
    let total_occurrences: u64 = typable.iter().map(|t| t.1).sum();
    let mut cumulative = Vec::with_capacity(typable.len());
    let mut sum = 0u64;
    for t in &typable {
        sum += t.1;
        cumulative.push(sum);
    }

    let mut rng = Rng(0x9E37_79B9_7F4A_7C15);
    let mut changed: HashMap<String, (u64, String)> = HashMap::new();
    let (mut kept, mut same_text, mut changed_total) = (0u64, 0u64, 0u64);
    let started = Instant::now();
    for _ in 0..sample {
        let pick = rng.next() % total_occurrences.max(1);
        let index = cumulative.partition_point(|c| *c <= pick).min(typable.len() - 1);
        let (token, _, keys, layout) = &typable[index];
        match env.decide(keys, *layout) {
            Verdict::Keep(_) => kept += 1,
            Verdict::Replace(r) if r.text == *token => same_text += 1,
            Verdict::Replace(r) => {
                changed_total += 1;
                changed.entry(token.clone()).or_insert((0, r.text)).0 += 1;
            }
        }
    }
    let elapsed = started.elapsed();
    println!("\n== replay of real code: {sample} tokens (a sample of the occurrences of {} distinct typable tokens) ==", typable.len());
    println!(
        "kept {kept}; replaced with the same text {same_text}; text CHANGED {changed_total} ({:.4}% of the tokens, {} distinct); {:.2} microseconds per decision",
        changed_total as f64 * 100.0 / sample as f64,
        changed.len(),
        elapsed.as_secs_f64() * 1e6 / sample as f64
    );
    let mut worst: Vec<_> = changed.iter().collect();
    worst.sort_by_key(|e| std::cmp::Reverse(e.1 .0));
    for (token, (n, text)) in worst.into_iter().take(8) {
        println!("  changed {n}x: {token} -> {text}");
    }
    Ok(changed_total as f64 * 100.0 / sample as f64 <= 0.01)
}

/// Ordinary words (comments, strings, names) typed correctly must not become code.
fn eval_prose(env: &Env, spec: &str) -> Result<bool, String> {
    let mut total = 0u64;
    let mut changed: Vec<String> = Vec::new();
    for (code, path) in spec.split(',').filter_map(|p| p.split_once('=')) {
        let alphabet: Vec<char> = match code.trim() {
            "ru" => "абвгдеёжзийклмнопрстуфхцчшщъыьэюя".chars().collect(),
            _ => "abcdefghijklmnopqrstuvwxyz".chars().collect(),
        };
        let layout = if code.trim() == "ru" { RU } else { EN };
        let words = crate::literary::read_counts(Path::new(path.trim()), &alphabet, &[])?;
        for (word, _) in words.iter().filter(|(w, _)| w.chars().count() >= 3).take(30_000) {
            let Some((keys, _)) = type_text(word) else { continue };
            total += 1;
            if let Verdict::Replace(r) = env.decide(&keys, layout) {
                if r.text != *word {
                    changed.push(format!("{word} -> {}", r.text));
                }
            }
        }
    }
    println!("\n== ordinary words typed correctly ({total} of the most frequent ones) ==");
    println!("text changed: {} ({:.4}%)", changed.len(), changed.len() as f64 * 100.0 / total.max(1) as f64);
    for example in changed.iter().take(8) {
        println!("  changed: {example}");
    }
    Ok(changed.len() as f64 * 100.0 / total.max(1) as f64 <= 0.05)
}

pub fn run(args: &Args) -> Result<(), String> {
    let packs = PathBuf::from(args.required("packs")?);
    let code = args.get("code").filter(|c| !c.is_empty()).unwrap_or("1c");
    let (infos, issues) = discover_dir(&packs, Kind::Programming);
    for issue in &issues {
        eprintln!("skipped {}: {}", issue.dir.display(), issue.message);
    }
    let info = infos.iter().find(|i| i.manifest.code == code).ok_or(format!("no pack '{code}' in {}", packs.display()))?;
    let pack = std::sync::Arc::new(load_pack(info)?);
    println!("{code}: {} words in the dictionary", pack.words.len());
    let render = table(&["en", "ru"]);
    let detector = ProgrammingDetector::new(pack, &render);
    let env = Env { detector: &detector, table: &render };

    let seed_ok = match args.get("seed").filter(|s| !s.is_empty()) {
        Some(seed) => eval_seed(&env, seed)?,
        None => true,
    };
    let replay_ok = match args.get("bsl").filter(|s| !s.is_empty()) {
        Some(root) => eval_replay(&env, root, args.number("sample", 1_000_000)?)?,
        None => true,
    };
    let prose_ok = match args.get("prose").filter(|s| !s.is_empty()) {
        Some(spec) => eval_prose(&env, spec)?,
        None => true,
    };
    let all_ok = seed_ok && replay_ok && prose_ok;
    println!(
        "\nthresholds (seed: 100% correct; replay of real code: text changed in <= 0.01% of the tokens; ordinary words <= 0.05%): {}",
        if all_ok { "met" } else { "NOT met" }
    );
    if args.flag("check") && !all_ok {
        return Err("the thresholds of the specification (7.2) are not met".into());
    }
    Ok(())
}
