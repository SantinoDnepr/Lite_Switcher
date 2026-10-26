//! Finding language packs, deciding which of them are active and opening their data.

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use lite_switcher_core::config::Config;
use lite_switcher_core::layout::Layout;
use lite_switcher_core::paths;

use crate::format::{self, Ngram, Words};
use crate::manifest::{Kind, Manifest};

pub const LITERARY_DIR: &str = "LiteraryLanguages";
pub const PROGRAMMING_DIR: &str = "ProgrammingLanguages";
const MANIFEST_FILE: &str = "pack.ini";

/// A pack found on the disk: only its manifest is read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackInfo {
    pub manifest: Manifest,
    pub dir: PathBuf,
}

/// A pack folder that was skipped, and why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Issue {
    pub dir: PathBuf,
    pub message: String,
}

/// Packs found in the root folder of one kind (`LiteraryLanguages` or `ProgrammingLanguages`).
pub fn discover_dir(root: &Path, kind: Kind) -> (Vec<PackInfo>, Vec<Issue>) {
    let mut packs = Vec::new();
    let mut issues = Vec::new();
    let Ok(entries) = fs::read_dir(root) else { return (packs, issues) };
    let mut dirs: Vec<PathBuf> = entries.flatten().map(|e| e.path()).filter(|p| p.is_dir()).collect();
    dirs.sort();
    for dir in dirs {
        let manifest_path = dir.join(MANIFEST_FILE);
        if !manifest_path.is_file() {
            continue; // not a pack
        }
        let name = dir.file_name().and_then(|n| n.to_str()).unwrap_or_default().to_string();
        let result = fs::read_to_string(&manifest_path)
            .map_err(|e| e.to_string())
            .and_then(|text| Manifest::parse(&text, Some(&name)).map_err(|e| e.to_string()))
            .and_then(|m| {
                if m.kind == kind {
                    Ok(m)
                } else {
                    Err(format!("the pack is {} but it is in the {} folder", m.kind.as_str(), kind.as_str()))
                }
            });
        match result {
            Ok(manifest) => packs.push(PackInfo { manifest, dir }),
            Err(message) => issues.push(Issue { dir, message }),
        }
    }
    packs.sort_by_key(|p| p.manifest.name.to_lowercase());
    (packs, issues)
}

/// Both kinds of packs next to the executables.
#[derive(Debug, Clone, Default)]
pub struct Discovered {
    pub literary: Vec<PackInfo>,
    pub programming: Vec<PackInfo>,
    pub issues: Vec<Issue>,
}

pub fn discover() -> Discovered {
    discover_in(paths::find_dir(LITERARY_DIR).as_deref(), paths::find_dir(PROGRAMMING_DIR).as_deref())
}

pub fn discover_in(literary_root: Option<&Path>, programming_root: Option<&Path>) -> Discovered {
    let mut found = Discovered::default();
    if let Some(root) = literary_root {
        let (packs, issues) = discover_dir(root, Kind::Literary);
        found.literary = packs;
        found.issues.extend(issues);
    }
    if let Some(root) = programming_root {
        let (packs, issues) = discover_dir(root, Kind::Programming);
        found.programming = packs;
        found.issues.extend(issues);
    }
    found
}

/// Does the layout id belong to the language `prefix` (`ru-RU` matches `ru`)?
pub fn id_matches(id: &str, prefix: &str) -> bool {
    let id = id.to_ascii_lowercase();
    let prefix = prefix.to_ascii_lowercase();
    id == prefix || id.starts_with(&format!("{prefix}-"))
}

/// Does an installed layout belong to the language `prefix` (`ru` matches `ru-RU`)?
fn layout_matches(layout: &Layout, prefix: &str) -> bool {
    id_matches(&layout.id, prefix)
}

/// A pack together with the answer to "does it work now?".
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackStatus {
    pub info: PackInfo,
    /// Literary: checked in the config. Programming: selected in the config.
    pub enabled: bool,
    /// Enabled and every layout that the pack needs is installed.
    pub active: bool,
    /// The layouts (language prefixes) that are needed but not installed.
    pub missing_layouts: Vec<String>,
}

/// Decides which packs are active for the installed layouts and the config.
///
/// Literary: enabled in `[literary_language]` and at least one layout of the language is installed.
/// Programming: selected in `[programming_language]` and **all** required layouts are installed.
pub fn evaluate(packs: &[PackInfo], installed: &[Layout], config: &Config) -> Vec<PackStatus> {
    packs
        .iter()
        .map(|info| {
            let m = &info.manifest;
            let (enabled, missing_layouts) = match m.kind {
                Kind::Literary => {
                    let missing = if m.layout_match.iter().any(|p| installed.iter().any(|l| layout_matches(l, p))) {
                        Vec::new()
                    } else {
                        m.layout_match.clone()
                    };
                    (config.literary_language.is_enabled(&m.code), missing)
                }
                Kind::Programming => {
                    let missing = m
                        .layout_require
                        .iter()
                        .filter(|p| !installed.iter().any(|l| layout_matches(l, p)))
                        .cloned()
                        .collect();
                    (config.programming_language.selected.eq_ignore_ascii_case(&m.code), missing)
                }
            };
            let active = enabled && missing_layouts.is_empty();
            PackStatus { info: info.clone(), enabled, active, missing_layouts }
        })
        .collect()
}

/// The opened data of an active pack.
pub struct LoadedPack {
    pub manifest: Manifest,
    pub words: Words,
    /// The user's own words of a programming pack (`local\words.fst` next to the manifest). It is
    /// built from the user's code, is never published and is optional: a broken file is ignored.
    pub local: Option<Words>,
    pub ngram: Option<Ngram>,
    pub exceptions: HashSet<String>,
}

impl LoadedPack {
    /// The value of a word in the pack, the published words first, then the local ones.
    pub fn get(&self, word: &str) -> Option<u32> {
        self.words.get(word).or_else(|| self.local.as_ref().and_then(|l| l.get(word)))
    }
}

/// The data of all active packs. Only these are in memory.
#[derive(Default)]
pub struct Loaded {
    pub literary: Vec<Arc<LoadedPack>>,
    /// The selected programming language (one).
    pub programming: Option<Arc<LoadedPack>>,
}

impl Loaded {
    /// Is there anything that the given mode could use?
    pub fn has_packs_for(&self, mode: lite_switcher_core::config::Mode) -> bool {
        use lite_switcher_core::config::Mode;
        match mode {
            Mode::Manual => true,
            Mode::AutoLiterary => !self.literary.is_empty(),
            Mode::AutoProgramming => self.programming.is_some(),
        }
    }
}

fn read_checked(dir: &Path, manifest: &Manifest, key: &str, file: &str) -> Result<Vec<u8>, String> {
    let path = dir.join(file);
    let bytes = fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    if let Some(expected) = manifest.files.sha256.get(key) {
        let actual = format::sha256_hex(&bytes);
        if &actual != expected {
            return Err(format!("{}: the checksum does not match the manifest", path.display()));
        }
    }
    Ok(bytes)
}

/// Opens the data of one pack. A checksum in the manifest is verified before the file is used.
pub fn load_pack(info: &PackInfo) -> Result<LoadedPack, String> {
    let m = &info.manifest;
    let words = if m.files.sha256.contains_key("words") {
        Words::from_bytes(read_checked(&info.dir, m, "words", &m.files.words)?).map_err(|e| e.to_string())?
    } else {
        Words::open(&info.dir.join(&m.files.words)).map_err(|e| e.to_string())?
    };
    let ngram = match &m.files.ngram {
        Some(file) => Some(Ngram::parse(&read_checked(&info.dir, m, "ngram", file)?).map_err(|e| e.to_string())?),
        None => None,
    };
    let exceptions = match &m.files.exceptions {
        Some(file) => format::parse_exceptions(&String::from_utf8_lossy(&read_checked(&info.dir, m, "exceptions", file)?)),
        None => HashSet::new(),
    };
    let local = (m.kind == Kind::Programming)
        .then(|| {
            // read, not mapped: dictgen can then rebuild the file while the daemon runs
            let bytes = fs::read(info.dir.join("local").join("words.fst")).ok()?;
            Words::from_bytes(bytes).ok()
        })
        .flatten();
    Ok(LoadedPack { manifest: m.clone(), words, local, ngram, exceptions })
}

/// Opens the data of the active packs; a pack that cannot be opened is skipped and reported.
pub fn load(statuses: &[PackStatus]) -> (Loaded, Vec<Issue>) {
    let mut loaded = Loaded::default();
    let mut issues = Vec::new();
    for status in statuses.iter().filter(|s| s.active) {
        match load_pack(&status.info) {
            Ok(pack) => match status.info.manifest.kind {
                Kind::Literary => loaded.literary.push(Arc::new(pack)),
                Kind::Programming => loaded.programming = Some(Arc::new(pack)),
            },
            Err(message) => issues.push(Issue { dir: status.info.dir.clone(), message }),
        }
    }
    (loaded, issues)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::tests::{LITERARY, PROGRAMMING};
    use lite_switcher_core::config::LiteraryLanguage;
    use std::sync::atomic::{AtomicU32, Ordering};

    static COUNTER: AtomicU32 = AtomicU32::new(0);

    /// A scratch folder with `LiteraryLanguages` and `ProgrammingLanguages` roots.
    struct Sandbox(PathBuf);

    impl Sandbox {
        fn new() -> Sandbox {
            let dir = std::env::temp_dir()
                .join(format!("ls-lang-packs-{}-{}", std::process::id(), COUNTER.fetch_add(1, Ordering::SeqCst)));
            fs::create_dir_all(dir.join(LITERARY_DIR)).unwrap();
            fs::create_dir_all(dir.join(PROGRAMMING_DIR)).unwrap();
            Sandbox(dir)
        }

        fn literary(&self) -> PathBuf {
            self.0.join(LITERARY_DIR)
        }

        fn programming(&self) -> PathBuf {
            self.0.join(PROGRAMMING_DIR)
        }

        fn discover(&self) -> Discovered {
            discover_in(Some(&self.literary()), Some(&self.programming()))
        }

        /// A literary pack with real data files.
        fn add_literary(&self, code: &str, name: &str) -> PathBuf {
            let dir = self.literary().join(code);
            fs::create_dir_all(&dir).unwrap();
            let text = LITERARY
                .replace("code = ru", &format!("code = {code}"))
                .replace("name = Russian", &format!("name = {name}"))
                .replace("match = ru", &format!("match = {code}"));
            fs::write(dir.join(MANIFEST_FILE), text).unwrap();
            fs::write(dir.join("words.fst"), Words::build([("слово".to_string(), 5)]).unwrap()).unwrap();
            let table = vec![10u8; 4 * 4 * 4];
            fs::write(dir.join("ngram3.bin"), Ngram::new(vec!['а', 'б', 'в'], table).unwrap().to_bytes()).unwrap();
            fs::write(dir.join("exceptions.txt"), "НАТО\n").unwrap();
            dir
        }

        fn add_programming(&self, code: &str) -> PathBuf {
            let dir = self.programming().join(code);
            fs::create_dir_all(&dir).unwrap();
            fs::write(dir.join(MANIFEST_FILE), PROGRAMMING.replace("code = 1c", &format!("code = {code}"))).unwrap();
            fs::write(dir.join("words.fst"), Words::build([("процедура".to_string(), flags_keyword())]).unwrap()).unwrap();
            dir
        }
    }

    fn flags_keyword() -> u32 {
        format::flags::KEYWORD
    }

    impl Drop for Sandbox {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn layout(id: &str) -> Layout {
        Layout { hkl: 0, id: id.into(), name: id.into() }
    }

    #[test]
    fn discovers_packs_and_reports_broken_ones() {
        let sb = Sandbox::new();
        sb.add_literary("ru", "Russian");
        sb.add_literary("uk", "Ukrainian");
        sb.add_programming("1c");
        // broken: code differs from the folder, a literary pack among the programming ones, no manifest
        let bad = sb.literary().join("xx");
        fs::create_dir_all(&bad).unwrap();
        fs::write(bad.join(MANIFEST_FILE), LITERARY).unwrap(); // code = ru in the folder xx
        let wrong_kind = sb.programming().join("ru");
        fs::create_dir_all(&wrong_kind).unwrap();
        fs::write(wrong_kind.join(MANIFEST_FILE), LITERARY).unwrap();
        fs::create_dir_all(sb.literary().join("just-a-folder")).unwrap();

        let found = sb.discover();
        let codes = |v: &[PackInfo]| v.iter().map(|p| p.manifest.code.clone()).collect::<Vec<_>>();
        assert_eq!(codes(&found.literary), ["ru", "uk"]);
        assert_eq!(codes(&found.programming), ["1c"]);
        assert_eq!(found.issues.len(), 2, "{:?}", found.issues);
        assert!(found.issues.iter().any(|i| i.message.contains("folder name")));
        assert!(found.issues.iter().any(|i| i.message.contains("literary but it is in")));
    }

    #[test]
    fn missing_roots_are_not_an_error() {
        let found = discover_in(None, Some(Path::new("no-such-folder")));
        assert!(found.literary.is_empty() && found.programming.is_empty() && found.issues.is_empty());
    }

    #[test]
    fn literary_pack_is_active_only_with_an_installed_layout() {
        let sb = Sandbox::new();
        for code in ["ru", "uk", "en", "de", "fr"] {
            sb.add_literary(code, code);
        }
        let found = sb.discover();
        let installed = [layout("en-US"), layout("ru-RU"), layout("uk-UA")];
        let st = evaluate(&found.literary, &installed, &Config::default());
        let active: Vec<&str> = st.iter().filter(|s| s.active).map(|s| s.info.manifest.code.as_str()).collect();
        assert_eq!(active, ["en", "ru", "uk"], "found 5 packs, 3 layouts installed: 3 are active");
        let de = st.iter().find(|s| s.info.manifest.code == "de").unwrap();
        assert!(de.enabled && !de.active);
        assert_eq!(de.missing_layouts, ["de"]);
    }

    #[test]
    fn literary_pack_follows_the_config_list() {
        let sb = Sandbox::new();
        sb.add_literary("ru", "Russian");
        sb.add_literary("en", "English");
        let config = Config {
            literary_language: LiteraryLanguage { enabled: Some(vec!["EN".into()]) },
            ..Config::default()
        };
        let st = evaluate(&sb.discover().literary, &[layout("en-US"), layout("ru-RU")], &config);
        let by = |code: &str| st.iter().find(|s| s.info.manifest.code == code).unwrap().clone();
        assert!(by("en").active);
        assert!(!by("ru").enabled && !by("ru").active);
    }

    #[test]
    fn programming_pack_needs_selection_and_all_layouts() {
        let sb = Sandbox::new();
        sb.add_programming("1c");
        let packs = sb.discover().programming;
        let mut config = Config::default();
        let both = [layout("en-US"), layout("ru-RU")];

        assert!(!evaluate(&packs, &both, &config)[0].active, "not selected");
        config.programming_language.selected = "1C".into();
        assert!(evaluate(&packs, &both, &config)[0].active);
        let only_en = evaluate(&packs, &[layout("en-US")], &config);
        assert!(only_en[0].enabled && !only_en[0].active);
        assert_eq!(only_en[0].missing_layouts, ["ru"]);
    }

    #[test]
    fn only_active_packs_are_loaded() {
        let sb = Sandbox::new();
        sb.add_literary("ru", "Russian");
        sb.add_literary("de", "German");
        sb.add_programming("1c");
        let mut config = Config::default();
        config.programming_language.selected = "1c".into();
        let found = sb.discover();
        let installed = [layout("en-US"), layout("ru-RU")];
        let mut statuses = evaluate(&found.literary, &installed, &config);
        statuses.extend(evaluate(&found.programming, &installed, &config));
        let (loaded, issues) = load(&statuses);
        assert!(issues.is_empty(), "{issues:?}");
        assert_eq!(loaded.literary.len(), 1, "the German pack has no layout: not opened");
        let ru = &loaded.literary[0];
        assert!(ru.words.contains("слово") && ru.exceptions.contains("нато") && ru.ngram.is_some());
        assert!(loaded.programming.as_ref().unwrap().words.contains("процедура"));
        assert!(loaded.has_packs_for(lite_switcher_core::config::Mode::AutoProgramming));

        // after the config changes the programming pack is not selected: nothing is loaded for it
        config.programming_language.selected.clear();
        let mut statuses = evaluate(&found.literary, &installed, &config);
        statuses.extend(evaluate(&found.programming, &installed, &config));
        let (loaded, _) = load(&statuses);
        assert!(loaded.programming.is_none() && !loaded.has_packs_for(lite_switcher_core::config::Mode::AutoProgramming));
    }

    #[test]
    fn local_lexicon_is_a_layer_of_a_programming_pack() {
        let sb = Sandbox::new();
        let dir = sb.add_programming("1c");
        let installed = [layout("en-US"), layout("ru-RU")];
        let mut config = lite_switcher_core::config::Config::default();
        config.programming_language.selected = "1c".into();
        let load_it = || {
            let found = sb.discover();
            let (loaded, issues) = load(&evaluate(&found.programming, &installed, &config));
            assert!(issues.is_empty(), "{issues:?}");
            loaded.programming.unwrap()
        };
        let pack = load_it();
        assert!(pack.local.is_none() && pack.get("своеимя").is_none() && pack.get("процедура").is_some());

        fs::create_dir_all(dir.join("local")).unwrap();
        fs::write(dir.join("local").join("words.fst"), Words::build([("своеимя".to_string(), format::flags::LOCAL)]).unwrap()).unwrap();
        let pack = load_it();
        assert!(pack.get("своеимя").is_some() && pack.get("процедура").is_some());
        assert!(!pack.words.contains("своеимя"), "the published words are not changed");

        // a broken local file is ignored, the pack still works
        fs::write(dir.join("local").join("words.fst"), b"garbage").unwrap();
        let pack = load_it();
        assert!(pack.local.is_none() && pack.get("процедура").is_some());
    }

    #[test]
    fn broken_data_skips_only_that_pack() {
        let sb = Sandbox::new();
        sb.add_literary("ru", "Russian");
        let uk = sb.add_literary("uk", "Ukrainian");
        fs::write(uk.join("words.fst"), b"garbage").unwrap();
        let installed = [layout("ru-RU"), layout("uk-UA")];
        let statuses = evaluate(&sb.discover().literary, &installed, &Config::default());
        let (loaded, issues) = load(&statuses);
        assert_eq!(loaded.literary.len(), 1);
        assert_eq!(issues.len(), 1);
        assert!(issues[0].dir.ends_with("uk"));
    }

    #[test]
    fn checksums_are_verified() {
        let sb = Sandbox::new();
        let dir = sb.add_literary("ru", "Russian");
        let good = format::sha256_hex(&fs::read(dir.join("words.fst")).unwrap());
        let manifest = fs::read_to_string(dir.join(MANIFEST_FILE)).unwrap();
        let with = |hash: &str| manifest.replace("exceptions = exceptions.txt\n", &format!("exceptions = exceptions.txt\nsha256_words = {hash}\n"));
        let installed = [layout("ru-RU")];

        fs::write(dir.join(MANIFEST_FILE), with(&good)).unwrap();
        let (loaded, issues) = load(&evaluate(&sb.discover().literary, &installed, &Config::default()));
        assert!(issues.is_empty() && loaded.literary.len() == 1);

        fs::write(dir.join(MANIFEST_FILE), with(&"0".repeat(64))).unwrap();
        let (loaded, issues) = load(&evaluate(&sb.discover().literary, &installed, &Config::default()));
        assert!(loaded.literary.is_empty());
        assert!(issues[0].message.contains("checksum"));
    }

    #[test]
    fn layout_prefix_matching() {
        assert!(layout_matches(&layout("ru-RU"), "ru"));
        assert!(layout_matches(&layout("RU-ru"), "ru"));
        assert!(layout_matches(&layout("ru"), "ru"));
        assert!(!layout_matches(&layout("rue-X"), "ru"), "a prefix must end at the dash");
        assert!(!layout_matches(&layout("en-US"), "ru"));
    }
}
