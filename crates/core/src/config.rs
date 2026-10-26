//! `config.ini` model: loading, saving and defaults. Sections: common, hotkeys, troubleshooting, sounds.

use std::path::{Path, PathBuf};
use std::{fs, io};

use ini::{Ini, ParseOption};

use crate::hotkey::Hotkey;

/// How the layout is switched: only by the hotkeys, or automatically.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Mode {
    /// Only the hotkeys convert text.
    #[default]
    Manual,
    /// Automatic switching of ordinary text (literary languages).
    AutoLiterary,
    /// Automatic switching of the keywords of the selected programming language.
    AutoProgramming,
}

impl Mode {
    pub const ALL: [Mode; 3] = [Mode::Manual, Mode::AutoLiterary, Mode::AutoProgramming];

    /// The value in `config.ini`.
    pub fn as_str(self) -> &'static str {
        match self {
            Mode::Manual => "manual",
            Mode::AutoLiterary => "auto_literary",
            Mode::AutoProgramming => "auto_programming",
        }
    }

    pub fn parse(s: &str) -> Option<Mode> {
        Mode::ALL.into_iter().find(|m| m.as_str().eq_ignore_ascii_case(s.trim()))
    }

    pub fn is_auto(self) -> bool {
        self != Mode::Manual
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Common {
    /// Layout ids (e.g. `en-US`) in cycle order. Empty = `auto` (all system layouts).
    pub layouts: Vec<String>,
    /// Layout ids excluded from the cycle when `layouts` is `auto`.
    pub exclude_layouts: Vec<String>,
    pub autostart: bool,
    pub show_tray_icon: bool,
    pub keep_trailing_space: bool,
    /// Manual / automatic switching.
    pub mode: Mode,
    /// Language of the texts: the code of a file in the `Language` folder, `en` = built-in English.
    pub language: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Hotkeys {
    /// Convert the last typed word.
    pub convert_word: Hotkey,
    /// Convert the selected text to the next layout. `None` = no hotkey.
    pub convert_selection: Option<Hotkey>,
    /// Invert the case of the selected text. `None` = no hotkey.
    pub invert_case: Option<Hotkey>,
    /// Turn the automatic switching on / off. `None` = no hotkey.
    pub toggle_autoswitch: Option<Hotkey>,
}

impl Hotkeys {
    pub fn default_convert_selection() -> Hotkey {
        Hotkey { shift: true, ..Hotkey::default() }
    }

    pub fn default_invert_case() -> Hotkey {
        Hotkey { alt: true, ..Hotkey::default() }
    }
}

/// Keys that make a word "unclean": if a checked key is pressed while a word is typed,
/// the automatic switching skips that word (the hotkeys always work).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NoSwitch {
    pub backspace: bool,
    pub left: bool,
    pub right: bool,
    pub up: bool,
    pub down: bool,
    pub delete: bool,
    pub change_layout: bool,
}

impl Default for NoSwitch {
    /// Everything is checked: a wrong switch is worse than a missed one.
    fn default() -> Self {
        NoSwitch { backspace: true, left: true, right: true, up: true, down: true, delete: true, change_layout: true }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Troubleshooting {
    pub log_enabled: bool,
    pub log_level: String,
    /// Executable names (e.g. `game.exe`) in which the daemon does nothing.
    pub ignored_apps: Vec<String>,
    pub key_delay_ms: u32,
    pub no_switch: NoSwitch,
}

/// Literary language packs that take part in the automatic switching.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct LiteraryLanguage {
    /// Pack codes (e.g. `ru`). `None` = `auto`: all found packs.
    pub enabled: Option<Vec<String>>,
}

impl LiteraryLanguage {
    pub fn is_enabled(&self, code: &str) -> bool {
        self.enabled.as_ref().is_none_or(|list| list.iter().any(|c| c.eq_ignore_ascii_case(code)))
    }
}

/// The programming language used by the `auto_programming` mode (exactly one).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ProgrammingLanguage {
    /// Pack code (e.g. `1c`); empty = nothing selected.
    pub selected: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Sounds {
    pub enabled: bool,
    /// Path to a .wav file; empty = system default sound.
    pub on_convert: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Config {
    pub common: Common,
    pub hotkeys: Hotkeys,
    pub troubleshooting: Troubleshooting,
    pub sounds: Sounds,
    pub literary_language: LiteraryLanguage,
    pub programming_language: ProgrammingLanguage,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            common: Common {
                layouts: Vec::new(),
                exclude_layouts: Vec::new(),
                autostart: false,
                show_tray_icon: true,
                keep_trailing_space: true,
                mode: Mode::Manual,
                language: "en".into(),
            },
            hotkeys: Hotkeys {
                convert_word: Hotkey::default(),
                convert_selection: Some(Hotkeys::default_convert_selection()),
                invert_case: Some(Hotkeys::default_invert_case()),
                toggle_autoswitch: None,
            },
            troubleshooting: Troubleshooting {
                log_enabled: false,
                log_level: "info".into(),
                ignored_apps: Vec::new(),
                key_delay_ms: 0,
                no_switch: NoSwitch::default(),
            },
            sounds: Sounds { enabled: false, on_convert: String::new() },
            literary_language: LiteraryLanguage::default(),
            programming_language: ProgrammingLanguage::default(),
        }
    }
}

fn parse_bool(s: &str) -> Option<bool> {
    match s.trim().to_ascii_lowercase().as_str() {
        "true" | "1" | "yes" | "on" => Some(true),
        "false" | "0" | "no" | "off" => Some(false),
        _ => None,
    }
}

fn parse_list(s: &str) -> Vec<String> {
    s.split(',').map(str::trim).filter(|x| !x.is_empty()).map(String::from).collect()
}

/// Empty value = no hotkey; an invalid value keeps the default.
fn parse_opt_hotkey(value: &str, default: Option<Hotkey>) -> Option<Hotkey> {
    let value = value.trim();
    if value.is_empty() {
        None
    } else {
        value.parse().ok().map_or(default, Some)
    }
}

fn opt_hotkey_string(hotkey: &Option<Hotkey>) -> String {
    hotkey.map(|h| h.to_string()).unwrap_or_default()
}

fn join(list: &[String]) -> String {
    list.join(",")
}

impl Config {
    /// `%APPDATA%\LiteSwitcher\config.ini`
    pub fn path() -> PathBuf {
        Config::dir().join("config.ini")
    }

    /// `%APPDATA%\LiteSwitcher`
    pub fn dir() -> PathBuf {
        let base = std::env::var_os("APPDATA").map(PathBuf::from).unwrap_or_else(std::env::temp_dir);
        base.join("LiteSwitcher")
    }

    /// Parses INI text; missing or invalid keys fall back to defaults.
    pub fn parse(text: &str) -> Result<Config, ini::ParseError> {
        let opt = ParseOption { enabled_escape: false, ..ParseOption::default() };
        let ini = Ini::load_from_str_opt(text, opt)?;
        let mut cfg = Config::default();
        let get = |section: &str, key: &str| ini.get_from(Some(section), key);

        if let Some(v) = get("common", "layouts") {
            cfg.common.layouts =
                if v.trim().eq_ignore_ascii_case("auto") { Vec::new() } else { parse_list(v) };
        }
        if let Some(v) = get("common", "exclude_layouts") {
            cfg.common.exclude_layouts = parse_list(v);
        }
        if let Some(v) = get("common", "autostart").and_then(parse_bool) {
            cfg.common.autostart = v;
        }
        if let Some(v) = get("common", "show_tray_icon").and_then(parse_bool) {
            cfg.common.show_tray_icon = v;
        }
        if let Some(v) = get("common", "keep_trailing_space").and_then(parse_bool) {
            cfg.common.keep_trailing_space = v;
        }
        // `mode` replaced the old `autoswitch` flag: it is read only when `mode` is absent
        if let Some(v) = get("common", "mode").and_then(Mode::parse) {
            cfg.common.mode = v;
        } else if get("common", "mode").is_none() && get("common", "autoswitch").and_then(parse_bool) == Some(true) {
            cfg.common.mode = Mode::AutoLiterary;
        }
        if let Some(v) = get("common", "language").map(str::trim).filter(|v| !v.is_empty()) {
            cfg.common.language = v.to_string();
        }
        if let Some(v) = get("hotkeys", "convert_word").and_then(|v| v.parse().ok()) {
            cfg.hotkeys.convert_word = v;
        }
        if let Some(v) = get("hotkeys", "convert_selection") {
            cfg.hotkeys.convert_selection = parse_opt_hotkey(v, cfg.hotkeys.convert_selection);
        }
        if let Some(v) = get("hotkeys", "invert_case") {
            cfg.hotkeys.invert_case = parse_opt_hotkey(v, cfg.hotkeys.invert_case);
        }
        if let Some(v) = get("hotkeys", "toggle_autoswitch") {
            cfg.hotkeys.toggle_autoswitch = parse_opt_hotkey(v, cfg.hotkeys.toggle_autoswitch);
        }
        if let Some(v) = get("troubleshooting", "log_enabled").and_then(parse_bool) {
            cfg.troubleshooting.log_enabled = v;
        }
        if let Some(v) = get("troubleshooting", "log_level") {
            cfg.troubleshooting.log_level = v.trim().to_ascii_lowercase();
        }
        if let Some(v) = get("troubleshooting", "ignored_apps") {
            cfg.troubleshooting.ignored_apps = parse_list(v);
        }
        if let Some(v) = get("troubleshooting", "key_delay_ms").and_then(|v| v.trim().parse().ok()) {
            cfg.troubleshooting.key_delay_ms = v;
        }
        let ns = &mut cfg.troubleshooting.no_switch;
        for (key, field) in [
            ("no_switch_if_backspace", &mut ns.backspace),
            ("no_switch_if_left", &mut ns.left),
            ("no_switch_if_right", &mut ns.right),
            ("no_switch_if_up", &mut ns.up),
            ("no_switch_if_down", &mut ns.down),
            ("no_switch_if_delete", &mut ns.delete),
            ("no_switch_if_change_layout", &mut ns.change_layout),
        ] {
            if let Some(v) = get("troubleshooting", key).and_then(parse_bool) {
                *field = v;
            }
        }
        if let Some(v) = get("literary_language", "enabled") {
            cfg.literary_language.enabled =
                if v.trim().eq_ignore_ascii_case("auto") { None } else { Some(parse_list(v)) };
        }
        if let Some(v) = get("programming_language", "selected") {
            cfg.programming_language.selected = v.trim().to_string();
        }
        if let Some(v) = get("sounds", "enabled").and_then(parse_bool) {
            cfg.sounds.enabled = v;
        }
        if let Some(v) = get("sounds", "on_convert") {
            cfg.sounds.on_convert = v.trim().to_string();
        }
        Ok(cfg)
    }

    /// Renders the config as INI text with explanatory comments.
    pub fn to_ini_string(&self) -> String {
        let c = &self.common;
        let t = &self.troubleshooting;
        let layouts = if c.layouts.is_empty() { "auto".to_string() } else { join(&c.layouts) };
        format!(
            "[common]
; auto = all keyboard layouts installed in Windows; otherwise a comma separated
; list of layout ids (e.g. en-US,ru-RU,uk-UA) - the order is the conversion cycle order
layouts = {layouts}
; layouts to skip when layouts = auto
exclude_layouts = {exclude}
; start the daemon with Windows
autostart = {autostart}
show_tray_icon = {tray}
; keep spaces typed after the word when converting
keep_trailing_space = {space}
; manual | auto_literary | auto_programming
mode = {mode}
; language of the texts: en (built in) or the code of a file in the Language folder (ru, uk, ...)
language = {language}

[hotkeys]
; convert the last typed word to the next layout (e.g. Pause, Ctrl+Pause, Shift+F9)
; Pause and Break are the same key
convert_word = {hotkey}
; convert the selected text to the next layout (empty = no hotkey)
convert_selection = {hk_selection}
; invert the case of the selected text (empty = no hotkey)
invert_case = {hk_case}
; turn the automatic switching on / off (empty = no hotkey)
toggle_autoswitch = {hk_toggle}

[troubleshooting]
log_enabled = {log}
; error | warn | info | debug
log_level = {level}
; comma separated executable names where the daemon does nothing (e.g. game.exe)
ignored_apps = {ignored}
; pause between synthetic key presses, for apps that lose input
key_delay_ms = {delay}
; the automatic switching skips a word if a checked key was pressed while it was typed
no_switch_if_backspace = {ns_backspace}
no_switch_if_left = {ns_left}
no_switch_if_right = {ns_right}
no_switch_if_up = {ns_up}
no_switch_if_down = {ns_down}
no_switch_if_delete = {ns_delete}
no_switch_if_change_layout = {ns_change_layout}

[literary_language]
; auto = all found language packs, otherwise pack codes separated by commas (e.g. ru,uk,en)
enabled = {literary}

[programming_language]
; code of the selected programming language pack (only one), e.g. 1c; empty = none
selected = {programming}

[sounds]
enabled = {snd}
; path to a .wav file; empty = system default sound
on_convert = {wav}
",
            exclude = join(&c.exclude_layouts),
            autostart = c.autostart,
            tray = c.show_tray_icon,
            space = c.keep_trailing_space,
            mode = c.mode.as_str(),
            language = c.language,
            hotkey = self.hotkeys.convert_word,
            hk_selection = opt_hotkey_string(&self.hotkeys.convert_selection),
            hk_case = opt_hotkey_string(&self.hotkeys.invert_case),
            hk_toggle = opt_hotkey_string(&self.hotkeys.toggle_autoswitch),
            log = t.log_enabled,
            level = t.log_level,
            ignored = join(&t.ignored_apps),
            delay = t.key_delay_ms,
            ns_backspace = t.no_switch.backspace,
            ns_left = t.no_switch.left,
            ns_right = t.no_switch.right,
            ns_up = t.no_switch.up,
            ns_down = t.no_switch.down,
            ns_delete = t.no_switch.delete,
            ns_change_layout = t.no_switch.change_layout,
            literary = self.literary_language.enabled.as_ref().map_or("auto".to_string(), |l| join(l)),
            programming = self.programming_language.selected,
            snd = self.sounds.enabled,
            wav = self.sounds.on_convert,
        )
    }

    pub fn load_from(path: &Path) -> Result<Config, String> {
        let text = fs::read_to_string(path).map_err(|e| e.to_string())?;
        Config::parse(text.trim_start_matches('\u{feff}')).map_err(|e| e.to_string())
    }

    /// Loads the config; if the file does not exist, writes the defaults and returns them.
    pub fn load_or_create() -> Result<Config, String> {
        let path = Config::path();
        if !path.exists() {
            let cfg = Config::default();
            cfg.save().map_err(|e| e.to_string())?;
            return Ok(cfg);
        }
        Config::load_from(&path)
    }

    pub fn save(&self) -> io::Result<()> {
        let path = Config::path();
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir)?;
        }
        fs::write(path, self.to_ini_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_roundtrip() {
        let cfg = Config::default();
        assert_eq!(Config::parse(&cfg.to_ini_string()).unwrap(), cfg);
    }

    #[test]
    fn custom_roundtrip_with_backslashes() {
        let mut cfg = Config::default();
        cfg.common.layouts = vec!["en-US".into(), "ru-RU".into(), "uk-UA".into()];
        cfg.common.exclude_layouts = vec!["de-DE".into()];
        cfg.common.autostart = true;
        cfg.common.mode = Mode::AutoProgramming;
        cfg.common.language = "uk".into();
        cfg.troubleshooting.no_switch.backspace = false;
        cfg.troubleshooting.no_switch.change_layout = false;
        cfg.literary_language.enabled = Some(vec!["ru".into(), "en".into()]);
        cfg.programming_language.selected = "1c".into();
        cfg.hotkeys.convert_word = "Ctrl+F9".parse().unwrap();
        cfg.hotkeys.convert_selection = None;
        cfg.hotkeys.invert_case = Some("Alt+F8".parse().unwrap());
        cfg.hotkeys.toggle_autoswitch = Some("Ctrl+Shift+A".parse().unwrap());
        cfg.troubleshooting.ignored_apps = vec!["game.exe".into(), "x.exe".into()];
        cfg.troubleshooting.key_delay_ms = 5;
        cfg.sounds.enabled = true;
        cfg.sounds.on_convert = r"C:\Sounds\click.wav".into();
        assert_eq!(Config::parse(&cfg.to_ini_string()).unwrap(), cfg);
    }

    #[test]
    fn missing_and_invalid_keys_use_defaults() {
        let cfg = Config::parse("[common]\nautostart = maybe\n[hotkeys]\nconvert_word = Nope\n").unwrap();
        assert_eq!(cfg, Config::default());
    }

    #[test]
    fn new_hotkeys_defaults() {
        let cfg = Config::default();
        assert_eq!(cfg.hotkeys.convert_selection.unwrap().to_string(), "Shift+Pause");
        assert_eq!(cfg.hotkeys.invert_case.unwrap().to_string(), "Alt+Pause");
        assert_eq!(cfg.hotkeys.toggle_autoswitch, None);
        assert_eq!(cfg.common.mode, Mode::Manual);
        // an empty value removes the hotkey, garbage keeps the default
        let cfg = Config::parse("[hotkeys]
convert_selection =
invert_case = Nope
").unwrap();
        assert_eq!(cfg.hotkeys.convert_selection, None);
        assert_eq!(cfg.hotkeys.invert_case, Some(Hotkeys::default_invert_case()));
    }

    #[test]
    fn mode_is_migrated_from_the_old_autoswitch_flag() {
        let old_on = Config::parse("[common]\nautoswitch = true\n").unwrap();
        assert_eq!(old_on.common.mode, Mode::AutoLiterary);
        let old_off = Config::parse("[common]\nautoswitch = false\n").unwrap();
        assert_eq!(old_off.common.mode, Mode::Manual);
        // `mode` wins over the old flag; an invalid mode falls back to manual
        let both = Config::parse("[common]\nmode = auto_programming\nautoswitch = true\n").unwrap();
        assert_eq!(both.common.mode, Mode::AutoProgramming);
        let bad = Config::parse("[common]\nmode = turbo\nautoswitch = true\n").unwrap();
        assert_eq!(bad.common.mode, Mode::Manual);
        // the old key is not written any more
        assert!(!Config::default().to_ini_string().contains("\nautoswitch ="));
        assert!(Config::default().to_ini_string().contains("mode = manual"));
    }

    #[test]
    fn no_switch_flags_default_to_checked() {
        let cfg = Config::default();
        assert_eq!(cfg.troubleshooting.no_switch, NoSwitch::default());
        assert!(cfg.troubleshooting.no_switch.backspace && cfg.troubleshooting.no_switch.change_layout);
        let cfg = Config::parse("[troubleshooting]\nno_switch_if_left = false\nno_switch_if_up = maybe\n").unwrap();
        assert!(!cfg.troubleshooting.no_switch.left);
        assert!(cfg.troubleshooting.no_switch.up, "an invalid value keeps the default");
    }

    #[test]
    fn language_sections() {
        let cfg = Config::default();
        assert_eq!(cfg.literary_language.enabled, None);
        assert!(cfg.literary_language.is_enabled("de"));
        assert_eq!(cfg.programming_language.selected, "");
        let cfg = Config::parse("[literary_language]\nenabled = ru, UK\n[programming_language]\nselected = 1c\n").unwrap();
        assert!(cfg.literary_language.is_enabled("uk") && !cfg.literary_language.is_enabled("en"));
        assert_eq!(cfg.programming_language.selected, "1c");
        let none = Config::parse("[literary_language]\nenabled =\n").unwrap();
        assert!(!none.literary_language.is_enabled("ru"), "an empty list means no languages");
    }
}
