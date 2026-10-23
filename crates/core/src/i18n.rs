//! Translations of the texts of the programs.
//!
//! English is built into the binaries (`Language/English.ini`, embedded at compile time) and is the
//! fallback for every other language. The other languages are `.ini` files in the `Language` folder
//! next to the executables; the language list of the settings is built from the files found there.
//!
//! A key is `group.key` (the section and the key of the INI file), e.g. `tray.exit`.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use ini::{Ini, ParseOption};

const ENGLISH_INI: &str = include_str!("../../../Language/English.ini");

/// Code of the built-in language.
pub const ENGLISH_CODE: &str = "en";

/// `group.key` -> text
type Entries = HashMap<String, String>;

fn parse(text: &str) -> Option<Entries> {
    let opt = ParseOption { enabled_escape: false, ..ParseOption::default() };
    let ini = Ini::load_from_str_opt(text.trim_start_matches('\u{feff}'), opt).ok()?;
    let mut entries = Entries::new();
    for (section, props) in ini.iter() {
        if let Some(section) = section {
            for (key, value) in props.iter() {
                entries.insert(format!("{section}.{key}"), value.trim().to_string());
            }
        }
    }
    Some(entries)
}

fn english() -> &'static Entries {
    static ENGLISH: OnceLock<Entries> = OnceLock::new();
    ENGLISH.get_or_init(|| parse(ENGLISH_INI).expect("the built-in English.ini is valid"))
}

fn read_file(path: &Path) -> Option<Entries> {
    parse(&fs::read_to_string(path).ok()?)
}

/// A language that can be selected.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Language {
    /// Value of `[common] language` in `config.ini`, e.g. `ru`.
    pub code: String,
    /// Name shown in the language list, e.g. `Russian`.
    pub name: String,
}

/// The `Language` folder: next to the executable or in one of the parent folders (so that it is
/// found in the project when the programs are run from `target\debug`).
pub fn language_dir() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    exe.parent()?.ancestors().take(5).map(|dir| dir.join("Language")).find(|dir| dir.is_dir())
}

/// Files of `dir` that look like translations: (code, name, entries).
fn language_files(dir: &Path) -> Vec<(String, String, Entries)> {
    let Ok(read_dir) = fs::read_dir(dir) else { return Vec::new() };
    let mut files = Vec::new();
    for entry in read_dir.flatten() {
        let path = entry.path();
        if !path.extension().is_some_and(|e| e.eq_ignore_ascii_case("ini")) {
            continue;
        }
        let Some(entries) = read_file(&path) else { continue };
        let code = entries.get("language.code").cloned().unwrap_or_default();
        if code.is_empty() {
            continue;
        }
        let name = entries.get("language.name").cloned().unwrap_or_else(|| code.clone());
        files.push((code, name, entries));
    }
    files
}

/// English plus the languages found in the `Language` folder.
pub fn available() -> Vec<Language> {
    available_in(language_dir().as_deref())
}

/// English (always first) plus the languages found in `dir`, sorted by name.
pub fn available_in(dir: Option<&Path>) -> Vec<Language> {
    let english_name = english().get("language.name").cloned().unwrap_or_else(|| "English".into());
    let mut others: Vec<Language> = Vec::new();
    for (code, name, _) in dir.map(language_files).unwrap_or_default() {
        let known = code.eq_ignore_ascii_case(ENGLISH_CODE) || others.iter().any(|l| l.code.eq_ignore_ascii_case(&code));
        if !known {
            others.push(Language { code, name });
        }
    }
    others.sort_by_key(|l| l.name.to_lowercase());
    let mut all = vec![Language { code: ENGLISH_CODE.into(), name: english_name }];
    all.extend(others);
    all
}

/// The texts of one language; missing texts come from the built-in English.
#[derive(Debug, Clone)]
pub struct Translator {
    code: String,
    entries: Entries,
}

impl Default for Translator {
    fn default() -> Self {
        Translator::english()
    }
}

impl Translator {
    /// The built-in English.
    pub fn english() -> Translator {
        Translator { code: ENGLISH_CODE.into(), entries: Entries::new() }
    }

    /// Loads the language from the `Language` folder; English if it is not found.
    pub fn new(code: &str) -> Translator {
        Translator::load(code, language_dir().as_deref())
    }

    pub fn load(code: &str, dir: Option<&Path>) -> Translator {
        let found = dir.map(language_files).unwrap_or_default().into_iter().find(|(c, _, _)| c.eq_ignore_ascii_case(code));
        match found {
            Some((code, _, entries)) => Translator { code, entries },
            None => Translator::english(),
        }
    }

    /// Code of the language that is really used (English if the requested one was not found).
    pub fn code(&self) -> &str {
        &self.code
    }

    fn lookup(&self, key: &str) -> Option<&str> {
        self.entries.get(key).or_else(|| english().get(key)).map(String::as_str)
    }

    /// The text for `group.key`; the key itself if there is no such text.
    pub fn t<'a>(&'a self, key: &'a str) -> &'a str {
        self.lookup(key).unwrap_or(key)
    }

    /// Like `t`, with `{name}` placeholders replaced.
    pub fn tf(&self, key: &str, args: &[(&str, &str)]) -> String {
        args.iter().fold(self.t(key).to_string(), |text, (name, value)| text.replace(&format!("{{{name}}}"), value))
    }

    /// Name of a keyboard layout by its locale id; `fallback` (the English name from Windows)
    /// if the language files do not know the layout.
    pub fn layout_name(&self, id: &str, fallback: &str) -> String {
        self.lookup(&format!("layouts.{id}")).unwrap_or(fallback).to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    fn project_dir() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
    }

    fn placeholders(text: &str) -> BTreeSet<String> {
        text.split('{').skip(1).filter_map(|s| s.split_once('}')).map(|(name, _)| name.to_string()).collect()
    }

    #[test]
    fn english_is_built_in() {
        let tr = Translator::english();
        assert_eq!(tr.t("tray.auto_switching"), "auto-switching");
        assert_eq!(tr.layout_name("en-US", "x"), "English (USA)");
        assert_eq!(tr.layout_name("ru-RU", "x"), "Russian (Russia)");
        assert_eq!(tr.layout_name("uk-UA", "x"), "Ukrainian (Ukraine)");
        assert_eq!(tr.layout_name("de-DE", "German (Germany)"), "German (Germany)");
        assert_eq!(tr.tf("status.duplicate_hotkey", &[("hotkey", "Ctrl+A")]), "The hotkey Ctrl+A is assigned to more than one action.");
    }

    #[test]
    fn missing_language_and_keys_fall_back() {
        let tr = Translator::load("xx", Some(&project_dir().join("Language")));
        assert_eq!(tr.code(), "en");
        assert_eq!(tr.t("tray.exit"), "Exit");
        assert_eq!(tr.t("no.such_key"), "no.such_key");
        assert_eq!(Translator::load("ru", None).code(), "en");
    }

    #[test]
    fn language_list_comes_from_the_folder() {
        let names: Vec<String> = available_in(Some(&project_dir().join("Language"))).into_iter().map(|l| l.name).collect();
        assert_eq!(names, ["English", "Russian", "Ukrainian"]);
        // without the folder English is still there
        assert_eq!(available_in(None), vec![Language { code: "en".into(), name: "English".into() }]);
        assert_eq!(available_in(Some(Path::new("no-such-dir"))).len(), 1);
    }

    #[test]
    fn languages_are_loaded_by_code() {
        let dir = project_dir().join("Language");
        assert_eq!(Translator::load("ru", Some(&dir)).t("tray.exit"), "Выход");
        assert_eq!(Translator::load("UK", Some(&dir)).t("tray.exit"), "Вихід");
        assert_eq!(Translator::load("uk", Some(&dir)).layout_name("uk-UA", "x"), "Українська (Україна)");
    }

    /// Every translation has exactly the texts of English.ini (new texts must be translated).
    #[test]
    fn translations_are_complete() {
        let english_keys: BTreeSet<&String> = english().keys().collect();
        let files = language_files(&project_dir().join("Language"));
        assert!(files.len() >= 3, "English, Russian and Ukrainian files are expected");
        for (code, _, entries) in files {
            let keys: BTreeSet<&String> = entries.keys().collect();
            let missing: Vec<_> = english_keys.difference(&keys).collect();
            let extra: Vec<_> = keys.difference(&english_keys).collect();
            assert!(missing.is_empty() && extra.is_empty(), "language {code}: missing {missing:?}, unknown {extra:?}");
            for (key, value) in &entries {
                assert!(!value.is_empty(), "language {code}: empty text {key}");
                assert_eq!(placeholders(value), placeholders(&english()[key]), "language {code}: placeholders of {key}");
            }
        }
    }

    /// Every text key written in the code (a string literal like "tray.exit") exists in English.ini.
    #[test]
    fn texts_used_in_the_code_exist() {
        let sections: BTreeSet<&str> = english().keys().filter_map(|k| k.split_once('.')).map(|(s, _)| s).collect();
        let mut checked = 0;
        for crate_dir in ["daemon", "settings"] {
            let src = project_dir().join("crates").join(crate_dir).join("src");
            for entry in fs::read_dir(src).unwrap().flatten() {
                let code = fs::read_to_string(entry.path()).unwrap();
                for section in &sections {
                    for part in code.split(&format!("\"{section}.")).skip(1) {
                        let key = format!("{section}.{}", part.split('"').next().unwrap());
                        assert!(english().contains_key(&key), "{}: unknown text {key}", entry.path().display());
                        checked += 1;
                    }
                }
            }
        }
        assert!(checked > 20, "only {checked} texts found");
    }
}
