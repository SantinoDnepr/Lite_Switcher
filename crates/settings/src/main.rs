#![windows_subsystem = "windows"]

use std::path::PathBuf;
use std::process::Command;

use eframe::egui;
use windows::core::w;
use windows::Win32::Foundation::{GetLastError, ERROR_ALREADY_EXISTS};
use windows::Win32::System::Threading::CreateMutexW;
use windows::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, VK_CONTROL, VK_LWIN, VK_MENU, VK_RWIN, VK_SHIFT};

use lite_switcher_core::config::{Config, Hotkeys, Mode};
use lite_switcher_core::hotkey::{vk_name, Hotkey};
use lite_switcher_core::i18n::{self, Language, Translator};
use lite_switcher_core::{autostart, layout};
use lite_switcher_lang::pack::{self, Discovered, PackStatus};

#[derive(Clone, Copy, PartialEq)]
enum Tab {
    Common,
    Literary,
    Programming,
    Hotkeys,
    Troubleshooting,
    Sounds,
}

/// The modes in the order of the drop-down list.
const MODES: [Mode; 3] = [Mode::AutoLiterary, Mode::AutoProgramming, Mode::Manual];

fn mode_text_key(mode: Mode) -> &'static str {
    match mode {
        Mode::AutoLiterary => "tray.mode_auto_literary",
        Mode::AutoProgramming => "tray.mode_auto_programming",
        Mode::Manual => "tray.mode_manual",
    }
}

/// The hotkeys that can be edited.
#[derive(Clone, Copy, PartialEq)]
enum HotkeyField {
    ConvertWord,
    ConvertSelection,
    InvertCase,
    ToggleAutoswitch,
}

impl HotkeyField {
    const ALL: [HotkeyField; 4] = [
        HotkeyField::ConvertWord,
        HotkeyField::ConvertSelection,
        HotkeyField::InvertCase,
        HotkeyField::ToggleAutoswitch,
    ];

    /// Key of the label in the language files.
    fn label_key(self) -> &'static str {
        match self {
            HotkeyField::ConvertWord => "hotkeys.convert_word",
            HotkeyField::ConvertSelection => "hotkeys.convert_selection",
            HotkeyField::InvertCase => "hotkeys.invert_case",
            HotkeyField::ToggleAutoswitch => "hotkeys.toggle_autoswitch",
        }
    }

    fn default_hotkey(self) -> Option<Hotkey> {
        match self {
            HotkeyField::ConvertWord => Some(Hotkey::default()),
            HotkeyField::ConvertSelection => Some(Hotkeys::default_convert_selection()),
            HotkeyField::InvertCase => Some(Hotkeys::default_invert_case()),
            HotkeyField::ToggleAutoswitch => None,
        }
    }

    /// The last word hotkey cannot be empty.
    fn optional(self) -> bool {
        self != HotkeyField::ConvertWord
    }
}

struct LayoutRow {
    id: String,
    name: String,
    enabled: bool,
    installed: bool,
}

/// The message under the buttons. Kept as a key, so that it is translated again when the language changes.
struct Status {
    key: &'static str,
    args: Vec<(&'static str, String)>,
}

struct App {
    cfg: Config,
    tab: Tab,
    auto_layouts: bool,
    rows: Vec<LayoutRow>,
    ignored_text: String,
    capturing: Option<HotkeyField>,
    status: Status,
    tr: Translator,
    /// English plus the languages found in the `Language` folder.
    languages: Vec<Language>,
    /// The window title has to be updated (the language changed).
    title_dirty: bool,
    /// Language packs found next to the program (only their manifests are read).
    packs: Discovered,
    /// All layouts installed in Windows (a pack needs some of them).
    installed: Vec<layout::Layout>,
    /// The check boxes of the literary packs, in the order of `packs.literary`.
    literary_checked: Vec<bool>,
}

fn daemon_exe() -> Option<PathBuf> {
    Some(std::env::current_exe().ok()?.with_file_name("lite-switcher.exe"))
}

fn is_down(vk: u32) -> bool {
    unsafe { GetAsyncKeyState(vk as i32) as u16 & 0x8000 != 0 }
}

impl App {
    fn new() -> App {
        let (cfg, status) = match Config::load_or_create() {
            Ok(cfg) => (cfg, Status { key: "status.config_path", args: vec![("path", Config::path().display().to_string())] }),
            Err(e) => (Config::default(), Status { key: "status.config_read_error_defaults", args: vec![("error", e)] }),
        };
        let tr = Translator::new(&cfg.common.language);
        let mut app = App {
            cfg,
            tab: Tab::Common,
            auto_layouts: true,
            rows: Vec::new(),
            ignored_text: String::new(),
            capturing: None,
            status,
            tr,
            languages: i18n::available(),
            title_dirty: false,
            packs: Discovered::default(),
            installed: Vec::new(),
            literary_checked: Vec::new(),
        };
        app.load_form();
        app.load_packs();
        app
    }

    /// Reads the manifests of the language packs and the installed layouts.
    fn load_packs(&mut self) {
        self.packs = pack::discover();
        self.installed = layout::enumerate();
        self.literary_checked =
            self.packs.literary.iter().map(|p| self.cfg.literary_language.is_enabled(&p.manifest.code)).collect();
    }

    /// The value of `[literary_language] enabled` for the check boxes: `None` (auto) when every found
    /// pack is checked; codes of packs that are not found now stay in the list.
    fn literary_enabled(&self) -> Option<Vec<String>> {
        let found: Vec<&str> = self.packs.literary.iter().map(|p| p.manifest.code.as_str()).collect();
        if found.is_empty() {
            return self.cfg.literary_language.enabled.clone();
        }
        let mut list: Vec<String> = found
            .iter()
            .zip(&self.literary_checked)
            .filter(|(_, checked)| **checked)
            .map(|(code, _)| code.to_string())
            .collect();
        let unknown: Vec<String> = match &self.cfg.literary_language.enabled {
            Some(saved) => saved.iter().filter(|c| !found.iter().any(|f| f.eq_ignore_ascii_case(c))).cloned().collect(),
            None => Vec::new(),
        };
        if list.len() == found.len() && unknown.is_empty() {
            return None;
        }
        list.extend(unknown);
        Some(list)
    }

    /// Which packs would be active with the current state of the form.
    fn pack_statuses(&self) -> (Vec<PackStatus>, Vec<PackStatus>) {
        let mut cfg = self.cfg.clone();
        cfg.literary_language.enabled = self.literary_enabled();
        (
            pack::evaluate(&self.packs.literary, &self.installed, &cfg),
            pack::evaluate(&self.packs.programming, &self.installed, &cfg),
        )
    }

    fn set_status(&mut self, key: &'static str, args: &[(&'static str, String)]) {
        self.status = Status { key, args: args.to_vec() };
    }

    /// Switches the texts of the window to the language selected in the config being edited.
    fn apply_language(&mut self) {
        self.tr = Translator::new(&self.cfg.common.language);
        self.title_dirty = true;
    }

    /// Fills the form state from `self.cfg` and the layouts installed in the system.
    fn load_form(&mut self) {
        let mut system: Vec<layout::Layout> = Vec::new();
        for l in layout::enumerate() {
            if !system.iter().any(|s| s.id.eq_ignore_ascii_case(&l.id)) {
                system.push(l);
            }
        }
        let common = &self.cfg.common;
        self.auto_layouts = common.layouts.is_empty();
        let mut rows: Vec<LayoutRow> = Vec::new();
        if self.auto_layouts {
            for l in &system {
                let excluded = common.exclude_layouts.iter().any(|e| e.eq_ignore_ascii_case(&l.id));
                rows.push(LayoutRow { id: l.id.clone(), name: l.name.clone(), enabled: !excluded, installed: true });
            }
        } else {
            for id in &common.layouts {
                let found = system.iter().find(|l| l.id.eq_ignore_ascii_case(id));
                rows.push(LayoutRow {
                    id: id.clone(),
                    name: found.map(|l| l.name.clone()).unwrap_or_default(),
                    enabled: true,
                    installed: found.is_some(),
                });
            }
            for l in &system {
                if !rows.iter().any(|r| r.id.eq_ignore_ascii_case(&l.id)) {
                    rows.push(LayoutRow { id: l.id.clone(), name: l.name.clone(), enabled: false, installed: true });
                }
            }
        }
        self.rows = rows;
        self.ignored_text = self.cfg.troubleshooting.ignored_apps.join("\n");
    }

    /// Copies the form state back to `self.cfg`.
    fn store_form(&mut self) {
        self.cfg.literary_language.enabled = self.literary_enabled();
        let common = &mut self.cfg.common;
        if self.auto_layouts {
            common.layouts = Vec::new();
            common.exclude_layouts = self.rows.iter().filter(|r| !r.enabled).map(|r| r.id.clone()).collect();
        } else {
            common.layouts = self.rows.iter().filter(|r| r.enabled).map(|r| r.id.clone()).collect();
            common.exclude_layouts = Vec::new();
        }
        self.cfg.troubleshooting.ignored_apps = self
            .ignored_text
            .split(['\n', ','])
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(String::from)
            .collect();
    }

    fn hotkey(&self, field: HotkeyField) -> Option<Hotkey> {
        let h = &self.cfg.hotkeys;
        match field {
            HotkeyField::ConvertWord => Some(h.convert_word),
            HotkeyField::ConvertSelection => h.convert_selection,
            HotkeyField::InvertCase => h.invert_case,
            HotkeyField::ToggleAutoswitch => h.toggle_autoswitch,
        }
    }

    fn set_hotkey(&mut self, field: HotkeyField, hotkey: Option<Hotkey>) {
        let h = &mut self.cfg.hotkeys;
        match field {
            HotkeyField::ConvertWord => h.convert_word = hotkey.unwrap_or_default(),
            HotkeyField::ConvertSelection => h.convert_selection = hotkey,
            HotkeyField::InvertCase => h.invert_case = hotkey,
            HotkeyField::ToggleAutoswitch => h.toggle_autoswitch = hotkey,
        }
    }

    /// Two actions must not share a hotkey.
    fn duplicate_hotkey(&self) -> Option<Hotkey> {
        let all: Vec<Hotkey> = HotkeyField::ALL.iter().filter_map(|f| self.hotkey(*f)).collect();
        all.iter().enumerate().find(|(i, h)| all[..*i].contains(h)).map(|(_, h)| *h)
    }

    fn save(&mut self) {
        self.store_form();
        if let Some(hotkey) = self.duplicate_hotkey() {
            self.set_status("status.duplicate_hotkey", &[("hotkey", hotkey.to_string())]);
            return;
        }
        if !self.auto_layouts && self.cfg.common.layouts.is_empty() {
            self.set_status("status.select_layout", &[]);
            return;
        }
        if let Err(e) = self.cfg.save() {
            self.set_status("status.save_error", &[("error", e.to_string())]);
            return;
        }
        self.set_status("status.saved", &[]);
        match daemon_exe() {
            Some(exe) if exe.exists() => {
                if let Err(e) = autostart::set(self.cfg.common.autostart, &exe) {
                    self.set_status("status.autostart_error", &[("error", e)]);
                }
            }
            _ if self.cfg.common.autostart => self.set_status("status.daemon_not_found", &[]),
            _ => {}
        }
    }

    fn reload(&mut self) {
        match Config::load_from(&Config::path()) {
            Ok(cfg) => {
                self.cfg = cfg;
                self.load_form();
                self.load_packs();
                self.apply_language();
                self.set_status("status.reloaded", &[]);
            }
            Err(e) => self.set_status("status.reload_error", &[("error", e)]),
        }
    }

    fn poll_hotkey_capture(&mut self, ctx: &egui::Context) {
        ctx.request_repaint();
        for vk in 0x08u32..=0xFE {
            if (0x10..=0x12).contains(&vk) || matches!(vk, 0x5B | 0x5C | 0xA0..=0xA5) {
                continue; // modifiers
            }
            if vk_name(vk).is_some() && is_down(vk) {
                if let Some(field) = self.capturing.take() {
                    self.set_hotkey(
                        field,
                        Some(Hotkey {
                            ctrl: is_down(VK_CONTROL.0 as u32),
                            alt: is_down(VK_MENU.0 as u32),
                            shift: is_down(VK_SHIFT.0 as u32),
                            win: is_down(VK_LWIN.0 as u32) || is_down(VK_RWIN.0 as u32),
                            vk,
                        }),
                    );
                }
                return;
            }
        }
    }

    fn common_tab(&mut self, ui: &mut egui::Ui) {
        let tr = &self.tr;
        ui.heading(tr.t("common.layouts_heading"));
        ui.checkbox(&mut self.auto_layouts, tr.t("common.all_layouts"));
        ui.label(if self.auto_layouts { tr.t("common.layouts_auto_hint") } else { tr.t("common.layouts_manual_hint") });
        let manual = !self.auto_layouts;
        let mut swap: Option<(usize, usize)> = None;
        let last = self.rows.len().saturating_sub(1);
        for (i, row) in self.rows.iter_mut().enumerate() {
            ui.horizontal(|ui| {
                ui.checkbox(&mut row.enabled, "");
                let mut label = format!("{}  {}", row.id, row.name);
                if !row.installed {
                    label.push_str(&format!("  {}", tr.t("common.not_installed")));
                }
                ui.label(label);
                if manual {
                    if ui.add_enabled(i > 0, egui::Button::new(tr.t("buttons.up"))).clicked() {
                        swap = Some((i, i - 1));
                    }
                    if ui.add_enabled(i < last, egui::Button::new(tr.t("buttons.down"))).clicked() {
                        swap = Some((i, i + 1));
                    }
                }
            });
        }
        if let Some((a, b)) = swap {
            self.rows.swap(a, b);
        }
        ui.separator();
        ui.heading(tr.t("common.general_heading"));
        ui.horizontal(|ui| {
            ui.label(tr.t("common.mode"));
            egui::ComboBox::from_id_salt("mode")
                .selected_text(tr.t(mode_text_key(self.cfg.common.mode)))
                .show_ui(ui, |ui| {
                    for mode in MODES {
                        ui.selectable_value(&mut self.cfg.common.mode, mode, tr.t(mode_text_key(mode)));
                    }
                });
        });
        let (literary, programming) = self.pack_statuses();
        let has_active_pack = match self.cfg.common.mode {
            Mode::Manual => true,
            Mode::AutoLiterary => literary.iter().any(|s| s.active),
            Mode::AutoProgramming => programming.iter().any(|s| s.active),
        };
        if !has_active_pack {
            ui.weak(tr.t("common.mode_hint_no_packs"));
        }
        ui.checkbox(&mut self.cfg.common.autostart, tr.t("common.autostart"));
        ui.checkbox(&mut self.cfg.common.show_tray_icon, tr.t("common.show_tray_icon"));
        ui.checkbox(&mut self.cfg.common.keep_trailing_space, tr.t("common.keep_trailing_space"));

        let current = &self.cfg.common.language;
        let current_name = self
            .languages
            .iter()
            .find(|l| l.code.eq_ignore_ascii_case(current))
            .map_or_else(|| current.clone(), |l| l.name.clone());
        let mut language_changed = false;
        ui.horizontal(|ui| {
            ui.label(tr.t("common.language"));
            egui::ComboBox::from_id_salt("language").selected_text(current_name).show_ui(ui, |ui| {
                for language in &self.languages {
                    language_changed |= ui
                        .selectable_value(&mut self.cfg.common.language, language.code.clone(), &language.name)
                        .changed();
                }
            });
        });
        if language_changed {
            self.apply_language();
        }
    }

    fn literary_tab(&mut self, ui: &mut egui::Ui) {
        let (statuses, _) = self.pack_statuses();
        let tr = &self.tr;
        ui.heading(tr.t("literary.heading"));
        let active = statuses.iter().filter(|s| s.active).count();
        ui.label(tr.tf("literary.summary", &[("found", &statuses.len().to_string()), ("active", &active.to_string())]));
        ui.weak(tr.t("literary.hint"));
        ui.add_space(4.0);
        for (checked, status) in self.literary_checked.iter_mut().zip(&statuses) {
            ui.horizontal(|ui| {
                ui.checkbox(checked, "");
                let m = &status.info.manifest;
                let label = format!("{} ({})", m.name, m.code);
                if status.enabled && !status.missing_layouts.is_empty() {
                    ui.weak(label);
                    ui.weak(format!("- {}", tr.t("packs.layout_missing")));
                } else {
                    ui.label(label);
                }
            });
        }
    }

    fn programming_tab(&mut self, ui: &mut egui::Ui) {
        let (_, statuses) = self.pack_statuses();
        let tr = &self.tr;
        ui.heading(tr.t("programming.heading"));
        ui.weak(tr.t("programming.hint"));
        ui.add_space(4.0);
        if self.packs.programming.is_empty() {
            ui.label(tr.t("programming.none_found"));
        }
        for status in &statuses {
            let m = &status.info.manifest;
            ui.horizontal(|ui| {
                ui.radio_value(&mut self.cfg.programming_language.selected, m.code.clone(), format!("{} ({})", m.name, m.code));
                ui.weak(tr.tf("packs.requires", &[("layouts", &m.layout_require.join(", "))]));
                if status.enabled && !status.missing_layouts.is_empty() {
                    ui.weak(format!("- {}", tr.t("packs.layout_missing")));
                }
            });
        }
    }

    fn hotkeys_tab(&mut self, ui: &mut egui::Ui) {
        ui.heading(self.tr.t("hotkeys.heading"));
        for field in HotkeyField::ALL {
            ui.horizontal(|ui| {
                ui.label(format!("{}:", self.tr.t(field.label_key())));
                if self.capturing == Some(field) {
                    ui.strong(self.tr.t("hotkeys.press_new"));
                    if ui.button(self.tr.t("buttons.cancel")).clicked() {
                        self.capturing = None;
                    }
                } else {
                    match self.hotkey(field) {
                        Some(hotkey) => ui.strong(hotkey.to_string()),
                        None => ui.weak(self.tr.t("hotkeys.not_set")),
                    };
                    if ui.button(self.tr.t("buttons.change")).clicked() {
                        self.capturing = Some(field);
                    }
                    if field.optional() && self.hotkey(field).is_some() && ui.button(self.tr.t("buttons.clear")).clicked() {
                        self.set_hotkey(field, None);
                    }
                    if self.hotkey(field) != field.default_hotkey() && ui.button(self.tr.t("buttons.default")).clicked() {
                        self.set_hotkey(field, field.default_hotkey());
                    }
                }
            });
        }
        ui.separator();
        ui.label(self.tr.t("hotkeys.note"));
    }

    fn troubleshooting_tab(&mut self, ui: &mut egui::Ui) {
        let tr = &self.tr;
        ui.heading(tr.t("troubleshooting.heading"));
        let t = &mut self.cfg.troubleshooting;
        ui.checkbox(&mut t.log_enabled, tr.t("troubleshooting.log_enabled"));
        ui.horizontal(|ui| {
            ui.label(tr.t("troubleshooting.log_level"));
            egui::ComboBox::from_id_salt("log_level").selected_text(t.log_level.clone()).show_ui(ui, |ui| {
                for level in ["error", "warn", "info", "debug"] {
                    ui.selectable_value(&mut t.log_level, level.to_string(), level);
                }
            });
        });
        ui.horizontal(|ui| {
            ui.label(tr.t("troubleshooting.key_delay"));
            ui.add(egui::DragValue::new(&mut t.key_delay_ms).range(0..=100));
        });
        ui.label(tr.t("troubleshooting.ignored_apps"));
        ui.add(egui::TextEdit::multiline(&mut self.ignored_text).desired_rows(4).desired_width(f32::INFINITY));
        ui.separator();
        ui.group(|ui| {
            ui.strong(tr.t("troubleshooting.no_switch_heading"));
            let ns = &mut t.no_switch;
            ui.checkbox(&mut ns.backspace, tr.t("troubleshooting.no_switch_backspace"));
            ui.checkbox(&mut ns.left, tr.t("troubleshooting.no_switch_left"));
            ui.checkbox(&mut ns.right, tr.t("troubleshooting.no_switch_right"));
            ui.checkbox(&mut ns.up, tr.t("troubleshooting.no_switch_up"));
            ui.checkbox(&mut ns.down, tr.t("troubleshooting.no_switch_down"));
            ui.checkbox(&mut ns.delete, tr.t("troubleshooting.no_switch_delete"));
            ui.checkbox(&mut ns.change_layout, tr.t("troubleshooting.no_switch_change_layout"));
            ui.weak(tr.t("troubleshooting.no_switch_hint"));
        });
    }

    fn sounds_tab(&mut self, ui: &mut egui::Ui) {
        ui.heading(self.tr.t("sounds.heading"));
        ui.checkbox(&mut self.cfg.sounds.enabled, self.tr.t("sounds.enabled"));
        ui.label(self.tr.t("sounds.path"));
        ui.add(egui::TextEdit::singleline(&mut self.cfg.sounds.on_convert).desired_width(f32::INFINITY));
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        if self.capturing.is_some() {
            let ctx = ui.ctx().clone();
            self.poll_hotkey_capture(&ctx);
        }
        if std::mem::take(&mut self.title_dirty) {
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::Title(self.tr.t("app.title").to_string()));
        }
        egui::Panel::bottom("bottom").show(ui, |ui| {
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                if ui.button(self.tr.t("buttons.save")).clicked() {
                    self.save();
                }
                if ui.button(self.tr.t("buttons.reload")).clicked() {
                    self.reload();
                }
                if ui.button(self.tr.t("buttons.open_config_folder")).clicked() {
                    let _ = Command::new("explorer").arg(Config::dir()).spawn();
                }
            });
            let args: Vec<(&str, &str)> = self.status.args.iter().map(|(n, v)| (*n, v.as_str())).collect();
            ui.label(self.tr.tf(self.status.key, &args));
            ui.add_space(4.0);
        });
        egui::CentralPanel::default().show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.selectable_value(&mut self.tab, Tab::Common, self.tr.t("tabs.common"));
                ui.selectable_value(&mut self.tab, Tab::Literary, self.tr.t("tabs.literary"));
                ui.selectable_value(&mut self.tab, Tab::Programming, self.tr.t("tabs.programming"));
                ui.selectable_value(&mut self.tab, Tab::Hotkeys, self.tr.t("tabs.hotkeys"));
                ui.selectable_value(&mut self.tab, Tab::Troubleshooting, self.tr.t("tabs.troubleshooting"));
                ui.selectable_value(&mut self.tab, Tab::Sounds, self.tr.t("tabs.sounds"));
            });
            ui.separator();
            egui::ScrollArea::vertical().show(ui, |ui| match self.tab {
                Tab::Common => self.common_tab(ui),
                Tab::Literary => self.literary_tab(ui),
                Tab::Programming => self.programming_tab(ui),
                Tab::Hotkeys => self.hotkeys_tab(ui),
                Tab::Troubleshooting => self.troubleshooting_tab(ui),
                Tab::Sounds => self.sounds_tab(ui),
            });
        });
    }
}

fn main() -> eframe::Result {
    // Single instance.
    let _mutex = unsafe {
        let handle = CreateMutexW(None, true, w!("Local\\LiteSwitcher.Settings"));
        if handle.is_err() || GetLastError() == ERROR_ALREADY_EXISTS {
            return Ok(());
        }
        handle
    };

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([520.0, 480.0]).with_min_inner_size([420.0, 360.0]),
        ..Default::default()
    };
    let app = App::new();
    let title = app.tr.t("app.title").to_string();
    eframe::run_native(&title, options, Box::new(|_cc| Ok(Box::new(app))))
}
