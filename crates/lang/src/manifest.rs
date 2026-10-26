//! `pack.ini`: the description of a language pack (see `docs/auto-switching-spec.md`, section 6.1).

use std::collections::HashMap;
use std::fmt;

use ini::{Ini, ParseOption};

/// The version of the data formats this build understands.
pub const SUPPORTED_FORMAT: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Literary,
    Programming,
}

impl Kind {
    pub fn as_str(self) -> &'static str {
        match self {
            Kind::Literary => "literary",
            Kind::Programming => "programming",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManifestError(pub String);

impl fmt::Display for ManifestError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for ManifestError {}

fn err<T>(message: impl Into<String>) -> Result<T, ManifestError> {
    Err(ManifestError(message.into()))
}

/// The files of a pack (plain file names inside the pack folder).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Files {
    pub words: String,
    pub ngram: Option<String>,
    pub exceptions: Option<String>,
    /// SHA-256 (hex, lower case) by the `[files]` key without the `sha256_` prefix: `words`, `ngram`...
    pub sha256: HashMap<String, String>,
}

/// Parameters of the literary detector.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LiteraryParams {
    pub alphabet: String,
    pub unique_letters: String,
    pub forbidden_bigrams: Vec<String>,
    pub min_word_len: usize,
    /// How much better than the current layout the best candidate has to be (no dictionary hit).
    pub switch_margin: u32,
    /// The same when the best candidate is a dictionary word.
    pub dict_margin: u32,
    /// Treat `ё` as `е` in the dictionary.
    pub fold_yo: bool,
}

impl Default for LiteraryParams {
    fn default() -> Self {
        LiteraryParams {
            alphabet: String::new(),
            unique_letters: String::new(),
            forbidden_bigrams: Vec::new(),
            min_word_len: 3,
            switch_margin: 10,
            dict_margin: 2,
            fold_yo: false,
        }
    }
}

/// Parameters of the programming language detector.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ProgrammingParams {
    pub case_sensitive: bool,
    pub sigils: Vec<String>,
    pub latin_segments: Vec<String>,
    /// Extra separator characters (besides the ones that are the same in every layout).
    pub separators: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Manifest {
    pub kind: Kind,
    /// Identifier of the pack = the name of its folder (`ru`, `1c`).
    pub code: String,
    /// The name shown in the settings.
    pub name: String,
    pub format: u32,
    pub version: u32,
    pub license: String,
    pub sources: Vec<String>,
    /// Literary: locale prefixes of the layouts of the language (`ru` matches `ru-RU`).
    pub layout_match: Vec<String>,
    /// Programming: layouts that all have to be installed.
    pub layout_require: Vec<String>,
    /// Programming: `(layout, role)`, e.g. `("ru", "native")`.
    pub layout_roles: Vec<(String, String)>,
    pub files: Files,
    pub literary: LiteraryParams,
    pub programming: ProgrammingParams,
}

fn list(value: &str) -> Vec<String> {
    value.split(',').map(str::trim).filter(|v| !v.is_empty()).map(String::from).collect()
}

fn parse_bool(value: &str) -> Option<bool> {
    match value.trim().to_ascii_lowercase().as_str() {
        "true" | "1" | "yes" | "on" => Some(true),
        "false" | "0" | "no" | "off" => Some(false),
        _ => None,
    }
}

/// A file name inside the pack folder: no folders, no `..`, no drive letters.
fn plain_file_name(name: &str) -> bool {
    !name.is_empty() && name != "." && name != ".." && !name.contains(['/', '\\', ':'])
}

impl Manifest {
    /// Parses and validates `pack.ini`. `dir_name` is the name of the pack folder: the `code` has to match it.
    pub fn parse(text: &str, dir_name: Option<&str>) -> Result<Manifest, ManifestError> {
        let opt = ParseOption { enabled_escape: false, ..ParseOption::default() };
        let ini = Ini::load_from_str_opt(text.trim_start_matches('\u{feff}'), opt)
            .map_err(|e| ManifestError(format!("not a valid INI file: {e}")))?;
        let get = |section: &str, key: &str| ini.get_from(Some(section), key).map(str::trim);
        let required = |section: &str, key: &str| match get(section, key) {
            Some(v) if !v.is_empty() => Ok(v.to_string()),
            _ => err(format!("[{section}] {key} is missing")),
        };
        let number = |key: &str| -> Result<u32, ManifestError> {
            required("pack", key)?.parse().map_err(|_| ManifestError(format!("[pack] {key} is not a number")))
        };

        let kind = match required("pack", "kind")?.to_ascii_lowercase().as_str() {
            "literary" => Kind::Literary,
            "programming" => Kind::Programming,
            other => return err(format!("[pack] kind '{other}' is not literary or programming")),
        };
        let code = required("pack", "code")?;
        if let Some(dir) = dir_name {
            if !dir.eq_ignore_ascii_case(&code) {
                return err(format!("[pack] code '{code}' differs from the folder name '{dir}'"));
            }
        }
        let format = number("format")?;
        if format != SUPPORTED_FORMAT {
            return err(format!("data format {format} is not supported (expected {SUPPORTED_FORMAT})"));
        }

        let layout_match = list(get("layouts", "match").unwrap_or_default());
        let layout_require = list(get("layouts", "require").unwrap_or_default());
        match kind {
            Kind::Literary if layout_match.is_empty() => return err("[layouts] match is missing"),
            Kind::Programming if layout_require.is_empty() => return err("[layouts] require is missing"),
            _ => {}
        }
        let layout_roles = list(get("layouts", "roles").unwrap_or_default())
            .into_iter()
            .filter_map(|r| r.split_once(':').map(|(l, role)| (l.trim().to_string(), role.trim().to_string())))
            .collect();

        let mut files = Files { words: required("files", "words")?, ..Files::default() };
        files.ngram = get("files", "ngram").filter(|v| !v.is_empty()).map(String::from);
        files.exceptions = get("files", "exceptions").filter(|v| !v.is_empty()).map(String::from);
        if kind == Kind::Literary && files.ngram.is_none() {
            return err("[files] ngram is missing");
        }
        for name in [Some(&files.words), files.ngram.as_ref(), files.exceptions.as_ref()].into_iter().flatten() {
            if !plain_file_name(name) {
                return err(format!("[files] '{name}' is not a plain file name"));
            }
        }
        if let Some(section) = ini.section(Some("files")) {
            for (key, value) in section.iter() {
                if let Some(name) = key.strip_prefix("sha256_") {
                    let hex = value.trim().to_ascii_lowercase();
                    if hex.len() != 64 || !hex.chars().all(|c| c.is_ascii_hexdigit()) {
                        return err(format!("[files] {key} is not a SHA-256 value"));
                    }
                    files.sha256.insert(name.to_string(), hex);
                }
            }
        }

        let mut literary = LiteraryParams::default();
        if let Some(v) = get("literary", "alphabet") {
            literary.alphabet = v.to_string();
        }
        if let Some(v) = get("literary", "unique_letters") {
            literary.unique_letters = v.to_string();
        }
        if let Some(v) = get("literary", "forbidden_bigrams") {
            literary.forbidden_bigrams = list(v);
        }
        if let Some(v) = get("literary", "min_word_len").and_then(|v| v.parse().ok()) {
            literary.min_word_len = v;
        }
        if let Some(v) = get("literary", "switch_margin").and_then(|v| v.parse().ok()) {
            literary.switch_margin = v;
        }
        if let Some(v) = get("literary", "dict_margin").and_then(|v| v.parse().ok()) {
            literary.dict_margin = v;
        }
        if let Some(v) = get("literary", "fold_yo").and_then(parse_bool) {
            literary.fold_yo = v;
        }

        let mut programming = ProgrammingParams::default();
        if let Some(v) = get("programming", "case_sensitive").and_then(parse_bool) {
            programming.case_sensitive = v;
        }
        if let Some(v) = get("programming", "sigils") {
            programming.sigils = list(v);
        }
        if let Some(v) = get("programming", "latin_segments") {
            programming.latin_segments = list(v);
        }
        if let Some(v) = get("programming", "separators") {
            programming.separators = v.to_string();
        }

        Ok(Manifest {
            kind,
            name: required("pack", "name")?,
            code,
            format,
            version: number("version")?,
            license: required("pack", "license")?,
            sources: list(&required("pack", "sources")?),
            layout_match,
            layout_require,
            layout_roles,
            files,
            literary,
            programming,
        })
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub const LITERARY: &str = "\
[pack]
kind = literary
code = ru
name = Russian
format = 1
version = 3
license = CC-BY-SA-4.0
sources = FrequencyWords-2018:ru

[layouts]
match = ru

[files]
words = words.fst
ngram = ngram3.bin
exceptions = exceptions.txt

[literary]
alphabet = абвгдеёжзийклмнопрстуфхцчшщъыьэюя
unique_letters = ыэъё
forbidden_bigrams = ъы, ьь
min_word_len = 4
switch_margin = 12
dict_margin = 3
fold_yo = true
";

    pub const PROGRAMMING: &str = "\
[pack]
kind = programming
code = 1c
name = 1C:Enterprise
format = 1
version = 1
license = CC-BY-SA-4.0
sources = seed, MCP

[layouts]
require = ru, en
roles = ru:native, en:latin

[files]
words = words.fst

[programming]
case_sensitive = false
sigils = #, &
latin_segments = Null, XML
";

    #[test]
    fn parses_a_literary_pack() {
        let m = Manifest::parse(LITERARY, Some("ru")).unwrap();
        assert_eq!((m.kind, m.code.as_str(), m.name.as_str(), m.version), (Kind::Literary, "ru", "Russian", 3));
        assert_eq!(m.layout_match, ["ru"]);
        assert_eq!(m.files.ngram.as_deref(), Some("ngram3.bin"));
        assert_eq!(m.literary.forbidden_bigrams, ["ъы", "ьь"]);
        assert_eq!((m.literary.min_word_len, m.literary.switch_margin, m.literary.dict_margin), (4, 12, 3));
        assert!(m.literary.fold_yo);
        assert_eq!(m.sources, ["FrequencyWords-2018:ru"]);
    }

    #[test]
    fn parses_a_programming_pack() {
        let m = Manifest::parse(PROGRAMMING, Some("1C")).unwrap();
        assert_eq!(m.kind, Kind::Programming);
        assert_eq!(m.layout_require, ["ru", "en"]);
        assert_eq!(m.layout_roles, [("ru".to_string(), "native".to_string()), ("en".to_string(), "latin".to_string())]);
        assert_eq!(m.programming.sigils, ["#", "&"]);
        assert_eq!(m.programming.latin_segments, ["Null", "XML"]);
        assert!(!m.programming.case_sensitive);
    }

    #[test]
    fn rejects_invalid_manifests() {
        let bad = |text: &str| Manifest::parse(text, Some("ru")).unwrap_err().0;
        assert!(bad("not an ini [").contains("INI"));
        assert!(bad(&LITERARY.replace("kind = literary", "kind = other")).contains("kind"));
        assert!(bad(&LITERARY.replace("code = ru", "code = uk")).contains("folder name"));
        assert!(bad(&LITERARY.replace("format = 1", "format = 7")).contains("not supported"));
        assert!(bad(&LITERARY.replace("version = 3", "version = x")).contains("version"));
        assert!(bad(&LITERARY.replace("license = CC-BY-SA-4.0\n", "")).contains("license"));
        assert!(bad(&LITERARY.replace("sources = FrequencyWords-2018:ru\n", "")).contains("sources"));
        assert!(bad(&LITERARY.replace("match = ru", "")).contains("match"));
        assert!(bad(&LITERARY.replace("ngram = ngram3.bin\n", "")).contains("ngram"));
        assert!(Manifest::parse(&PROGRAMMING.replace("require = ru, en", ""), Some("1c")).unwrap_err().0.contains("require"));
    }

    #[test]
    fn file_names_cannot_leave_the_pack_folder() {
        for evil in ["..\\evil.fst", "../evil.fst", "C:evil.fst", "sub/words.fst", ".."] {
            let text = LITERARY.replace("words = words.fst", &format!("words = {evil}"));
            assert!(Manifest::parse(&text, Some("ru")).is_err(), "{evil} must be rejected");
        }
    }

    #[test]
    fn reads_checksums() {
        let hash = "a".repeat(64);
        let text = LITERARY.replace("exceptions = exceptions.txt\n", &format!("exceptions = exceptions.txt\nsha256_words = {}\n", hash.to_uppercase()));
        assert_eq!(Manifest::parse(&text, None).unwrap().files.sha256.get("words"), Some(&hash));
        let text = LITERARY.replace("exceptions = exceptions.txt\n", "exceptions = exceptions.txt\nsha256_words = 1234\n");
        assert!(Manifest::parse(&text, None).is_err());
    }
}
