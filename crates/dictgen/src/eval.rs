//! `dictgen eval`: how well do the literary packs recognize words typed in the wrong layout?
//!
//! For every test word the keys are typed in the right layout (the detector must keep the word) and
//! in the other layouts (the detector must restore the word). The words come from the same
//! frequency lists the packs were built from: the frequent ones are in the dictionaries, the rare
//! ones that are not in a dictionary show how the n-grams work alone.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use lite_switcher_lang::literary::{Context, LiteraryDetector, Verdict};
use lite_switcher_lang::manifest::Kind;
use lite_switcher_lang::pack::{discover_dir, load_pack, Loaded};
use lite_switcher_lang::render::presets::{table, try_keys_for};
use lite_switcher_lang::render::RenderTable;

use crate::literary::read_counts;
use crate::Args;

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
}

fn shuffled(mut words: Vec<String>, seed: u64) -> Vec<String> {
    let mut rng = Rng(seed | 1);
    for i in (1..words.len()).rev() {
        words.swap(i, (rng.next() % (i as u64 + 1)) as usize);
    }
    words
}

#[derive(Default)]
struct Stats {
    kept_ok: u64,
    false_switches: u64,
    /// Words typed in a wrong layout whose text differs: [restored, total] by length bucket.
    by_len: [[u64; 2]; 4],
    /// The same by (true language, layout it was typed in).
    pairs: HashMap<(usize, usize), [u64; 2]>,
    /// The text is the same in both layouts (ru/uk): only the layout has to be fixed.
    layout_only: [u64; 2],
    false_examples: Vec<String>,
    missed_examples: Vec<String>,
    /// Why the words were not restored: reason -> count.
    miss_reasons: HashMap<String, u64>,
    /// Restored with the right text, but in the other language with the same text (ru/uk): [count, total].
    text_only: u64,
    /// False switches by the length of the word.
    false_by_len: [u64; 4],
    /// False switches of words with 5 or more letters.
    false_long_examples: Vec<String>,
}

fn bucket(len: usize) -> usize {
    match len {
        0..=3 => 0,
        4 => 1,
        5..=6 => 2,
        _ => 3,
    }
}

fn pct(part: u64, total: u64) -> f64 {
    if total == 0 { 100.0 } else { part as f64 * 100.0 / total as f64 }
}

struct Env<'a> {
    detector: &'a LiteraryDetector,
    table: &'a RenderTable,
    codes: &'a [String],
}

fn run_words(env: &Env, lang: usize, words: &[String], stats: &mut Stats, decisions: &mut (u64, f64)) {
    let code = &env.codes[lang];
    for word in words {
        let Some(keys) = try_keys_for(word, code) else { continue };
        let mut decide = |current: usize| {
            let started = Instant::now();
            let verdict = env.detector.decide(env.table, &Context { keys: &keys, current, previous: None });
            decisions.0 += 1;
            decisions.1 += started.elapsed().as_secs_f64();
            verdict
        };
        // typed in the right layout: must be kept
        match decide(lang) {
            Verdict::Keep(_) => stats.kept_ok += 1,
            Verdict::Replace(r) => {
                stats.false_switches += 1;
                stats.false_by_len[bucket(word.chars().count())] += 1;
                if stats.false_examples.len() < 15 {
                    stats.false_examples.push(format!("{word} ({code}) -> {} [{}]", r.text, env.codes[r.layout]));
                }
                if word.chars().count() >= 5 && stats.false_long_examples.len() < 12 {
                    stats.false_long_examples.push(format!("{word} ({code}) -> {} [{}]", r.text, env.codes[r.layout]));
                }
            }
        }
        // typed in another layout: must be restored
        for other in 0..env.codes.len() {
            if other == lang {
                continue;
            }
            let Some(shown) = env.table.render_word(&keys, other) else { continue };
            let verdict = decide(other);
            if shown == *word {
                stats.layout_only[1] += 1;
                if matches!(&verdict, Verdict::Replace(r) if r.layout == lang) {
                    stats.layout_only[0] += 1;
                }
                continue;
            }
            let restored = matches!(&verdict, Verdict::Replace(r) if r.text == *word);
            if matches!(&verdict, Verdict::Replace(r) if r.text == *word && r.layout != lang) {
                stats.text_only += 1;
            }
            let b = bucket(word.chars().count());
            stats.by_len[b][1] += 1;
            stats.pairs.entry((lang, other)).or_default()[1] += 1;
            if restored {
                stats.by_len[b][0] += 1;
                stats.pairs.entry((lang, other)).or_default()[0] += 1;
            } else {
                let reason = match &verdict {
                    Verdict::Keep(r) => format!("kept: {r:?}"),
                    Verdict::Replace(r) if r.layout != lang => "replaced with another language".to_string(),
                    Verdict::Replace(_) => "replaced with a different text".to_string(),
                };
                *stats.miss_reasons.entry(reason).or_default() += 1;
                if stats.missed_examples.len() < 15 {
                    stats.missed_examples.push(format!("{word} typed in {} shows {shown}: {}", env.codes[other], describe(&verdict)));
                }
            }
        }
    }
}

fn describe(verdict: &Verdict) -> String {
    match verdict {
        Verdict::Keep(reason) => format!("kept ({reason:?})"),
        Verdict::Replace(r) => format!("replaced with {}", r.text),
    }
}

/// Prints the results and returns whether the specification thresholds (7.2) are met.
fn report(title: &str, env: &Env, stats: &Stats, head: bool) -> bool {
    let total_correct = stats.kept_ok + stats.false_switches;
    let fp = pct(stats.false_switches, total_correct);
    let restored: u64 = stats.by_len.iter().map(|b| b[0]).sum();
    let total: u64 = stats.by_len.iter().map(|b| b[1]).sum();
    let lens: Vec<f64> = stats.by_len.iter().map(|b| pct(b[0], b[1])).collect();
    println!("\n== {title} ==");
    println!(
        "false switches {fp:.3}% ({}/{total_correct}); text restored {:.1}% ({restored}/{total}); by length 3:{:.1}% 4:{:.1}% 5-6:{:.1}% 7+:{:.1}%; same-text cases (ru/uk) switched anyway: {} of {}",
        stats.false_switches,
        pct(restored, total),
        lens[0], lens[1], lens[2], lens[3],
        stats.layout_only[0],
        stats.layout_only[1],
    );
    let mut pairs: Vec<_> = stats.pairs.iter().collect();
    pairs.sort();
    let pair_text: Vec<String> =
        pairs.iter().map(|((a, b), v)| format!("{}<-{}:{:.0}%", env.codes[*a], env.codes[*b], pct(v[0], v[1]))).collect();
    println!("pairs (true language <- typed in): {}", pair_text.join(" "));
    println!(
        "the text was restored in a layout of another language with the same text: {} ({:.1}% of the restored)",
        stats.text_only,
        pct(stats.text_only, restored)
    );
    println!(
        "false switches by length 3:{} 4:{} 5-6:{} 7+:{}",
        stats.false_by_len[0], stats.false_by_len[1], stats.false_by_len[2], stats.false_by_len[3]
    );
    for example in &stats.false_long_examples {
        println!("  false switch (5+ letters): {example}");
    }
    let mut reasons: Vec<_> = stats.miss_reasons.iter().collect();
    reasons.sort_by(|a, b| b.1.cmp(a.1));
    let reason_text: Vec<String> = reasons.iter().map(|(r, n)| format!("{r}: {n}")).collect();
    println!("not restored because: {}", reason_text.join("; "));
    for example in stats.false_examples.iter().take(8) {
        println!("  false switch: {example}");
    }
    for example in stats.missed_examples.iter().take(8) {
        println!("  missed: {example}");
    }
    // ru <-> uk share most letters, so that pair is much harder than the others
    let pair_min = |same_family: bool| {
        stats
            .pairs
            .iter()
            .filter(|((a, b), _)| (env.codes[*a] != "en" && env.codes[*b] != "en") == same_family)
            .map(|(_, v)| pct(v[0], v[1]))
            .fold(100.0, f64::min)
    };
    let (ok, thresholds) = if head {
        (
            fp <= 0.05
                && pct(restored, total) >= 95.0
                && lens[0] >= 80.0
                && lens[1] >= 90.0
                && lens[2] >= 95.0
                && lens[3] >= 95.0
                && pair_min(false) >= 95.0
                && pair_min(true) >= 65.0,
            "false <= 0.05%, restored >= 95%, 3 letters >= 80%, 4 >= 90%, 5+ >= 95%, en<->ru/uk pairs >= 95%, ru<->uk >= 65%",
        )
    } else {
        (fp <= 0.2 && lens[2] >= 65.0 && lens[3] >= 65.0, "false <= 0.2%, 5+ letters >= 65%")
    };
    println!("thresholds ({thresholds}): {}", if ok { "met" } else { "NOT met" });
    ok
}

pub fn run(args: &Args) -> Result<(), String> {
    let packs_dir = PathBuf::from(args.required("packs")?);
    let sample: usize = args.number("sample", 5000)?;
    let head_rank: usize = args.number("head-rank", 30_000)?;
    let switch_margin: Option<u32> = args.get("switch-margin").and_then(|v| v.parse().ok());
    let dict_margin: Option<u32> = args.get("dict-margin").and_then(|v| v.parse().ok());
    let max_cost: Option<u32> = args.get("max-cost").and_then(|v| v.parse().ok());
    let langs: Vec<(String, PathBuf)> = args
        .required("freq")?
        .split(',')
        .filter_map(|p| p.split_once('='))
        .map(|(code, path)| (code.trim().to_string(), PathBuf::from(path.trim())))
        .collect();
    if langs.len() < 2 {
        return Err("--freq needs at least two code=file pairs".into());
    }
    let codes: Vec<String> = langs.iter().map(|l| l.0.clone()).collect();
    if let Some(bad) = codes.iter().find(|c| !["en", "ru", "uk"].contains(&c.as_str())) {
        return Err(format!("the preset layouts support en, ru and uk, not '{bad}'"));
    }

    let (infos, issues) = discover_dir(&packs_dir, Kind::Literary);
    for issue in &issues {
        eprintln!("skipped {}: {}", issue.dir.display(), issue.message);
    }
    let mut packs = Vec::new();
    for code in &codes {
        let info = infos.iter().find(|i| i.manifest.code == *code).ok_or(format!("no pack '{code}' in {}", packs_dir.display()))?;
        let mut pack = load_pack(info)?;
        if let Some(v) = switch_margin {
            pack.manifest.literary.switch_margin = v;
        }
        if let Some(v) = dict_margin {
            pack.manifest.literary.dict_margin = v;
        }
        if let Some(v) = max_cost {
            pack.manifest.literary.max_average_cost = v;
        }
        println!(
            "{code}: dictionary {} words, switch margin {}, dictionary margin {}, max average cost {}",
            pack.words.len(),
            pack.manifest.literary.switch_margin,
            pack.manifest.literary.dict_margin,
            pack.manifest.literary.max_average_cost
        );
        packs.push(Arc::new(pack));
    }
    let loaded = Loaded { literary: packs.clone(), programming: None };
    let ids: Vec<&str> = codes.iter().map(String::as_str).collect();
    let render = table(&ids);
    let detector = LiteraryDetector::new(&loaded, render.layout_ids());
    let env = Env { detector: &detector, table: &render, codes: &codes };

    let mut head_stats = Stats::default();
    let mut tail_stats = Stats::default();
    let mut apostrophe = Stats::default();
    let mut decisions = (0u64, 0f64);
    for (lang, (code, path)) in langs.iter().enumerate() {
        let pack = &packs[lang];
        let alphabet: Vec<char> = pack.manifest.literary.alphabet.chars().collect();
        let all = read_counts(path, &alphabet, &['\''])?;
        let pure = |w: &str| w.chars().all(|c| alphabet.contains(&c)) && (3..=12).contains(&w.chars().count());
        let ranked: Vec<&(String, u64)> = all.iter().filter(|(w, _)| pure(w)).collect();
        let head: Vec<String> = ranked.iter().take(head_rank).map(|(w, _)| w.clone()).collect();
        let tail: Vec<String> = ranked
            .iter()
            .skip(head_rank)
            .filter(|(w, c)| *c >= 2 && !pack.words.contains(w))
            .map(|(w, _)| w.clone())
            .collect();
        let with_apostrophe: Vec<String> = all
            .iter()
            .filter(|(w, c)| *c >= 20 && w.contains('\'') && (3..=12).contains(&w.chars().count()))
            .map(|(w, _)| w.clone())
            .collect();
        let take = |v: Vec<String>, seed: u64| shuffled(v, seed).into_iter().take(sample).collect::<Vec<_>>();
        println!("{code}: {} head words, {} unseen tail words, {} words with an apostrophe", head.len(), tail.len(), with_apostrophe.len());
        run_words(&env, lang, &take(head, 11 + lang as u64), &mut head_stats, &mut decisions);
        run_words(&env, lang, &take(tail, 23 + lang as u64), &mut tail_stats, &mut decisions);
        if !with_apostrophe.is_empty() {
            // only the "must be kept" half is meaningful for these words
            run_words(&env, lang, &take(with_apostrophe, 37), &mut apostrophe, &mut decisions);
        }
    }

    let head_ok = report("frequent words (in the dictionaries)", &env, &head_stats, true);
    let tail_ok = report("rare words that are not in a dictionary", &env, &tail_stats, false);
    if apostrophe.kept_ok + apostrophe.false_switches > 0 {
        let total = apostrophe.kept_ok + apostrophe.false_switches;
        println!(
            "\nwords with an apostrophe typed correctly: {} of {total} switched wrongly ({:.2}%)",
            apostrophe.false_switches,
            pct(apostrophe.false_switches, total)
        );
        for example in apostrophe.false_examples.iter().take(5) {
            println!("  false switch: {example}");
        }
    }
    println!("\n{} decisions, {:.2} microseconds each", decisions.0, decisions.1 * 1e6 / decisions.0.max(1) as f64);
    if args.flag("check") && !(head_ok && tail_ok) {
        return Err("the thresholds of the specification (7.2) are not met".into());
    }
    Ok(())
}
