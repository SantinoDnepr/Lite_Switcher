//! `dictgen programming`: the dictionary of a programming language from a seed list and the `.bsl`
//! files of real configurations.
//!
//! The seed is the language itself (keywords, directives, query language, both forms). The `.bsl`
//! files add what is used in real code. The corpus is private: only tokens that are used in at
//! least `--min-groups` independent configurations (projects that share most of their files count
//! as one) are written, so that identifiers of one company do not get into the published pack.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

use lite_switcher_lang::build::frequency_class;
use lite_switcher_lang::format::{flags, sha256_hex, Words};

use crate::Args;

/// A letter of the languages of the pack (Latin or Cyrillic).
fn is_letter(c: char) -> bool {
    c.is_ascii_alphabetic() || ('\u{0400}'..='\u{04FF}').contains(&c)
}

fn is_cyrillic(c: char) -> bool {
    ('\u{0400}'..='\u{04FF}').contains(&c)
}

/// The tokens of the code: comments, strings and the continuation lines of multi-line strings are
/// skipped. A token is a word, with `#` or `&` in front of it for preprocessor and directives.
pub fn tokens(text: &str, mut each: impl FnMut(String)) {
    for line in text.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("//") || trimmed.starts_with('|') {
            continue;
        }
        let chars: Vec<char> = line.chars().collect();
        let mut i = 0;
        let mut in_string = false;
        while i < chars.len() {
            let c = chars[i];
            if in_string {
                if c == '"' {
                    if chars.get(i + 1) == Some(&'"') {
                        i += 1;
                    } else {
                        in_string = false;
                    }
                }
                i += 1;
                continue;
            }
            if c == '"' {
                in_string = true;
                i += 1;
                continue;
            }
            if c == '/' && chars.get(i + 1) == Some(&'/') {
                break;
            }
            let sigil = (c == '#' || c == '&') && chars.get(i + 1).is_some_and(|n| is_letter(*n) || *n == '_');
            if is_letter(c) || c == '_' || sigil {
                let start = i;
                i += 1;
                while i < chars.len() && (is_letter(chars[i]) || chars[i] == '_' || chars[i].is_ascii_digit() || (sigil && i == start + 1)) {
                    i += 1;
                }
                each(chars[start..i].iter().collect::<String>().to_lowercase());
                continue;
            }
            i += 1;
        }
    }
}

/// `<russian form>|<english form>|<flags>` lines; a form may be empty.
pub fn parse_seed(text: &str) -> Result<Vec<(String, u32)>, String> {
    let mut entries = Vec::new();
    for (number, line) in text.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with(';') {
            continue;
        }
        let parts: Vec<&str> = line.split('|').collect();
        if parts.len() != 3 {
            return Err(format!("seed line {}: expected <ru>|<en>|<flags>", number + 1));
        }
        let mut bits = 0;
        for flag in parts[2].split(',').map(str::trim).filter(|f| !f.is_empty()) {
            bits |= match flag {
                "keyword" => flags::KEYWORD,
                "directive" => flags::DIRECTIVE,
                "preprocessor" => flags::PREPROCESSOR,
                "platform" => flags::PLATFORM,
                "hybrid" => flags::HYBRID,
                "no_convert" => flags::NO_CONVERT,
                other => return Err(format!("seed line {}: unknown flag '{other}'", number + 1)),
            };
        }
        for form in &parts[..2] {
            let form = form.trim();
            if !form.is_empty() {
                entries.push((form.to_lowercase(), bits | 60_000));
            }
        }
    }
    Ok(entries)
}

/// The tokens of a corpus with the number of independent groups that use them.
pub struct Corpus {
    /// token -> (occurrences in unique files, number of independent groups)
    pub tokens: HashMap<String, (u64, usize)>,
    pub files: usize,
    pub unique_files: usize,
    pub projects: usize,
    pub groups: usize,
}

fn bsl_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            bsl_files(&path, out);
        } else if path.extension().is_some_and(|e| e.eq_ignore_ascii_case("bsl")) {
            out.push(path);
        }
    }
}

/// Reads the `.bsl` files under `root`. The project of a file is the folder `depth` levels below
/// `root`; projects whose files overlap by at least `overlap` (of the smaller project) are one group.
pub fn mine(root: &Path, depth: usize, overlap: f64) -> Result<Corpus, String> {
    let mut files = Vec::new();
    bsl_files(root, &mut files);
    if files.is_empty() {
        return Err(format!("no .bsl files under {}", root.display()));
    }
    let mut project_files: HashMap<String, HashSet<String>> = HashMap::new();
    let mut content_tokens: HashMap<String, HashMap<String, u32>> = HashMap::new();
    for path in &files {
        let Ok(bytes) = fs::read(path) else { continue };
        let hash = sha256_hex(&bytes);
        let relative = path.strip_prefix(root).unwrap_or(path);
        let project: Vec<String> =
            relative.components().take(depth).map(|c| c.as_os_str().to_string_lossy().into_owned()).collect();
        project_files.entry(project.join("/")).or_default().insert(hash.clone());
        content_tokens.entry(hash).or_insert_with(|| {
            let text = String::from_utf8_lossy(&bytes);
            let mut counts: HashMap<String, u32> = HashMap::new();
            tokens(text.trim_start_matches('\u{feff}'), |t| *counts.entry(t).or_default() += 1);
            counts
        });
    }

    // groups of projects: a union of the projects that share files
    let names: Vec<&String> = project_files.keys().collect();
    let mut parent: Vec<usize> = (0..names.len()).collect();
    fn find(parent: &mut [usize], x: usize) -> usize {
        let mut root = x;
        while parent[root] != root {
            root = parent[root];
        }
        let mut node = x;
        while parent[node] != root {
            let next = parent[node];
            parent[node] = root;
            node = next;
        }
        root
    }
    for i in 0..names.len() {
        for j in i + 1..names.len() {
            let (a, b) = (&project_files[names[i]], &project_files[names[j]]);
            let shared = a.intersection(b).count() as f64;
            if shared / a.len().min(b.len()).max(1) as f64 >= overlap {
                let (ri, rj) = (find(&mut parent, i), find(&mut parent, j));
                parent[ri] = rj;
            }
        }
    }
    let group_of: HashMap<&String, usize> = (0..names.len()).map(|i| (names[i], find(&mut parent, i))).collect();
    let groups: HashSet<usize> = group_of.values().copied().collect();

    // the groups that contain a file, and the tokens of the unique files
    let mut file_groups: HashMap<&String, HashSet<usize>> = HashMap::new();
    for (project, hashes) in &project_files {
        for hash in hashes {
            file_groups.entry(hash).or_default().insert(group_of[project]);
        }
    }
    let mut totals: HashMap<String, (u64, HashSet<usize>)> = HashMap::new();
    for (hash, counts) in &content_tokens {
        let g = &file_groups[hash];
        for (token, n) in counts {
            let entry = totals.entry(token.clone()).or_default();
            entry.0 += u64::from(*n);
            entry.1.extend(g.iter().copied());
        }
    }
    Ok(Corpus {
        tokens: totals.into_iter().map(|(t, (n, g))| (t, (n, g.len()))).collect(),
        files: files.len(),
        unique_files: content_tokens.len(),
        projects: project_files.len(),
        groups: groups.len(),
    })
}

fn list(value: Option<&str>) -> String {
    value.unwrap_or_default().split(',').map(str::trim).filter(|v| !v.is_empty()).collect::<Vec<_>>().join(",")
}

/// The local lexicon: the frequent tokens of the user's own code, for the machine of the user only.
/// It is a layer on top of the published pack (`local\words.fst` in the pack folder, never committed).
fn run_local(args: &Args) -> Result<(), String> {
    let out = PathBuf::from(args.required("out")?);
    let dir = args.required("bsl")?;
    let min_count: u64 = args.number("min-count", 3)?;
    let depth: usize = args.number("project-depth", 2)?;
    let overlap: f64 = args.number("cluster-overlap", 0.5)?;
    let corpus = mine(Path::new(dir), depth, overlap)?;
    let entries: Vec<(String, u32)> = corpus
        .tokens
        .iter()
        .filter(|(token, (count, _))| {
            let body = token.trim_start_matches(['#', '&']);
            // short tokens are never replaced by the detector, so they are not stored
            *count >= min_count && body.chars().filter(|c| is_letter(*c)).count() >= 4 && body.chars().count() <= 40
        })
        .map(|(token, (count, _))| (token.clone(), frequency_class(*count) | flags::LOCAL))
        .collect();
    fs::create_dir_all(&out).map_err(|e| format!("{}: {e}", out.display()))?;
    let words = Words::build(entries).map_err(|e| e.to_string())?;
    fs::write(out.join("words.fst"), &words).map_err(|e| e.to_string())?;
    let total = Words::from_bytes(words.clone()).map_err(|e| e.to_string())?.len();
    println!("local lexicon: {total} words from {} files, words.fst {} bytes in {}", corpus.files, words.len(), out.display());
    Ok(())
}

pub fn run(args: &Args) -> Result<(), String> {
    if args.flag("local") {
        return run_local(args);
    }
    let code = args.required("code")?;
    let name = args.required("name")?;
    let out = PathBuf::from(args.required("out")?);
    let license = args.required("license")?;
    let sources = args.required("sources")?;
    let seed_text = fs::read_to_string(args.required("seed")?).map_err(|e| e.to_string())?;
    let min_groups: usize = args.number("min-groups", 3)?;
    let min_count: u64 = args.number("min-count", 20)?;
    let depth: usize = args.number("project-depth", 2)?;
    let overlap: f64 = args.number("cluster-overlap", 0.5)?;
    let require = list(args.get("require").or(Some("ru,en")));
    let roles = args.get("roles").unwrap_or("ru:native, en:latin");
    let sigils = list(args.get("sigils").or(Some("#,&")));
    let segments = list(args.get("latin-segments").or(Some(
        "Null,XML,JSON,XDTO,HTML,SMS,IBAN,HTTP,DOM,URI,SMTP,Windows,SQL,ADO,CML,COM,SOAP,FTP,IMAP,POP3,XSD,CSV,ZIP,OLE,ODBC",
    )));

    let seed = parse_seed(&seed_text)?;
    let mut entries: Vec<(String, u32)> = seed.clone();
    let seed_keys: HashSet<&str> = seed.iter().map(|(w, _)| w.as_str()).collect();
    let mut mined = 0usize;
    if let Some(path) = args.get("platform").filter(|p| !p.is_empty()) {
        // the names of the platform (mcp-dump): they are public API names, the layer under the mined ones
        let mut added = 0usize;
        for name in crate::mcp::read_names(Path::new(path))? {
            let key = name.to_lowercase();
            if key.chars().filter(|c| is_letter(*c)).count() >= 2 && !seed_keys.contains(key.as_str()) {
                entries.push((key, 40_000 | flags::PLATFORM));
                added += 1;
            }
        }
        println!("platform: {added} names from {path}");
    }
    if let Some(dir) = args.get("bsl").filter(|d| !d.is_empty()) {
        let corpus = mine(Path::new(dir), depth, overlap)?;
        println!(
            "corpus: {} files ({} unique), {} projects in {} independent groups, {} distinct tokens",
            corpus.files, corpus.unique_files, corpus.projects, corpus.groups, corpus.tokens.len()
        );
        for (token, (count, groups)) in &corpus.tokens {
            let body = token.trim_start_matches(['#', '&']);
            let letters = body.chars().filter(|c| is_letter(*c)).count();
            if seed_keys.contains(token.as_str()) || *groups < min_groups || *count < min_count || letters < 2 || body.chars().count() > 40 {
                continue;
            }
            let hybrid = body.chars().any(is_cyrillic) && body.chars().any(|c| c.is_ascii_alphabetic());
            entries.push((token.clone(), frequency_class(*count) | if hybrid { flags::HYBRID } else { 0 }));
            mined += 1;
        }
        println!("kept {mined} tokens used in at least {min_groups} independent groups, {min_count} times or more");
    }

    fs::create_dir_all(&out).map_err(|e| format!("{}: {e}", out.display()))?;
    let words = Words::build(entries).map_err(|e| e.to_string())?;
    crate::check_size("words.fst", &words)?;
    fs::write(out.join("words.fst"), &words).map_err(|e| e.to_string())?;
    let manifest = format!(
        "[pack]\nkind = programming\ncode = {code}\nname = {name}\nformat = 1\nversion = 1\nlicense = {license}\nsources = {sources}\n\n\
         [layouts]\nrequire = {require}\nroles = {roles}\n\n[files]\nwords = words.fst\nsha256_words = {}\n\n\
         [programming]\ncase_sensitive = false\nmin_token_len = 3\nsigils = {sigils}\nlatin_segments = {segments}\n",
        sha256_hex(&words)
    );
    fs::write(out.join("pack.ini"), manifest).map_err(|e| e.to_string())?;
    let total = Words::from_bytes(words.clone()).map_err(|e| e.to_string())?.len();
    println!("{code}: {total} words ({} from the seed, {mined} mined), words.fst {} bytes", seed.len(), words.len());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn all_tokens(text: &str) -> Vec<String> {
        let mut out = Vec::new();
        tokens(text, |t| out.push(t));
        out
    }

    #[test]
    fn tokenizer_skips_comments_and_strings() {
        let code = "#Область Тест\n&НаКлиенте\nПроцедура Тест() Экспорт\n  Если Х = \"текст \"\"в строке\"\" и еще\" Тогда // комментарий Слово\n  Запрос.Текст = \"ВЫБРАТЬ\n  |Таблица.Поле\n  |ИЗ Справочник\";\nКонецПроцедуры\n// Только комментарий\nЕстьNull(a_1, b2)";
        let t = all_tokens(code);
        assert_eq!(t[..2], ["#область", "тест"]);
        assert!(t.contains(&"&наклиенте".to_string()) && t.contains(&"процедура".to_string()));
        assert!(t.contains(&"тогда".to_string()) && t.contains(&"запрос".to_string()) && t.contains(&"текст".to_string()));
        for skipped in ["слово", "строке", "выбрать", "таблица", "справочник", "комментарий", "только"] {
            assert!(!t.contains(&skipped.to_string()), "{skipped} must be skipped");
        }
        assert!(t.contains(&"естьnull".to_string()) && t.contains(&"a_1".to_string()) && t.contains(&"b2".to_string()));
        // `&` or `#` that is not followed by a letter is not part of a token
        assert_eq!(all_tokens("a & b # 1"), ["a", "b"]);
    }

    #[test]
    fn seed_lines_are_parsed() {
        let seed = parse_seed("; comment\nПроцедура|Procedure|keyword\n&НаКлиенте|&AtClient|directive\nЕстьNull||hybrid\nNull||keyword\n").unwrap();
        let keys: Vec<&str> = seed.iter().map(|(w, _)| w.as_str()).collect();
        assert_eq!(keys, ["процедура", "procedure", "&наклиенте", "&atclient", "естьnull", "null"]);
        assert_ne!(seed[4].1 & flags::HYBRID, 0);
        assert_ne!(seed[2].1 & flags::DIRECTIVE, 0);
        assert!(parse_seed("one|two").is_err());
        assert!(parse_seed("a|b|nonsense").is_err());
    }

    #[test]
    fn mining_counts_independent_groups_only() {
        let dir = std::env::temp_dir().join(format!("ls-dictgen-mine-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let write = |path: &str, text: &str| {
            let p = dir.join(path);
            fs::create_dir_all(p.parent().unwrap()).unwrap();
            fs::write(p, text).unwrap();
        };
        // a and a_copy are one configuration (identical files), b and c are different ones
        for project in ["a", "a_copy"] {
            write(&format!("{project}/m1.bsl"), "Процедура Общая()\nКонецПроцедуры\nСвоеИмя1()");
            write(&format!("{project}/m2.bsl"), "Общая()");
        }
        write("b/m.bsl", "Процедура Общая()\nКонецПроцедуры\nСвоеИмя2()");
        write("c/m.bsl", "Общая()\nСвоеИмя3()");
        let corpus = mine(&dir, 1, 0.5).unwrap();
        assert_eq!((corpus.projects, corpus.groups), (4, 3));
        assert_eq!(corpus.tokens["общая"].1, 3, "used in three independent groups");
        assert_eq!(corpus.tokens["своеимя1"].1, 1, "a and its copy are one group");
        assert_eq!(corpus.unique_files, 4);
        fs::remove_dir_all(&dir).unwrap();
    }

    fn run_with(extra: &[&str]) -> Result<(), String> {
        let argv: Vec<String> = extra.iter().map(|s| s.to_string()).collect();
        run(&Args::parse(&argv)?)
    }

    #[test]
    fn the_public_pack_has_no_private_identifiers_and_the_local_one_has_them() {
        let root = std::env::temp_dir().join(format!("ls-dictgen-public-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let code = root.join("code");
        for (project, text) in [
            ("a", "Процедура ОбщееИмя()
СвоеИмяКомпании()
КонецПроцедуры"),
            ("b", "Процедура ОбщееИмя()
КонецПроцедуры"),
            ("c", "ОбщееИмя()"),
        ] {
            fs::create_dir_all(code.join(project)).unwrap();
            fs::write(code.join(project).join("m.bsl"), text).unwrap();
        }
        fs::write(root.join("seed.txt"), "Процедура|Procedure|keyword
").unwrap();
        let path = |p: &str| root.join(p).to_string_lossy().into_owned();

        run_with(&[
            "--code", "t", "--name", "T", "--seed", &path("seed.txt"), "--out", &path("public"), "--license", "x",
            "--sources", "x", "--bsl", &path("code"), "--min-groups", "3", "--min-count", "1", "--project-depth", "1",
        ])
        .unwrap();
        let public = Words::open(&root.join("public").join("words.fst")).unwrap();
        assert!(public.contains("общееимя") && public.contains("процедура"));
        assert!(!public.contains("своеимякомпании"), "an identifier of one project must not be published");

        run_with(&["--local", "--bsl", &path("code"), "--out", &path("local"), "--min-count", "1", "--project-depth", "1"]).unwrap();
        let local = Words::open(&root.join("local").join("words.fst")).unwrap();
        assert!(local.contains("своеимякомпании") && local.get("своеимякомпании").unwrap() & flags::LOCAL != 0);
        drop((public, local));
        fs::remove_dir_all(&root).unwrap();
    }
}
