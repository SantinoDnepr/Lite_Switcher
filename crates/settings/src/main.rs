#![windows_subsystem = "windows"]

use std::path::PathBuf;
use std::process::Command;

use eframe::egui;
use windows::core::w;
use windows::Win32::Foundation::{GetLastError, ERROR_ALREADY_EXISTS};
use windows::Win32::System::Threading::CreateMutexW;
use windows::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, VK_CONTROL, VK_LWIN, VK_MENU, VK_RWIN, VK_SHIFT};

use lite_switcher_core::config::{Config, Hotkeys};
use lite_switcher_core::hotkey::{vk_name, Hotkey};
use lite_switcher_core::{autostart, layout};

#[derive(Clone, Copy, PartialEq)]
enum Tab {
    Common,
    Hotkeys,
    Troubleshooting,
    Sounds,
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

    fn label(self) -> &'static str {
        match self {
            HotkeyField::ConvertWord => "Convert the last word",
            HotkeyField::ConvertSelection => "Convert the selected text",
            HotkeyField::InvertCase => "Invert the case of the selected text",
            HotkeyField::ToggleAutoswitch => "Turn the autoswitch on / off",
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

struct App {
    cfg: Config,
    tab: Tab,
    auto_layouts: bool,
    rows: Vec<LayoutRow>,
    ignored_text: String,
    capturing: Option<HotkeyField>,
    status: String,
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
            Ok(cfg) => (cfg, format!("Config: {}", Config::path().display())),
            Err(e) => (Config::default(), format!("Cannot read the config, defaults are shown: {e}")),
        };
        let mut app = App {
            cfg,
            tab: Tab::Common,
            auto_layouts: true,
            rows: Vec::new(),
            ignored_text: String::new(),
            capturing: None,
            status,
        };
        app.load_form();
        app
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
            self.status = format!("The hotkey {hotkey} is assigned to more than one action.");
            return;
        }
        if !self.auto_layouts && self.cfg.common.layouts.is_empty() {
            self.status = "Select at least one layout (or enable \"All system layouts\").".into();
            return;
        }
        if let Err(e) = self.cfg.save() {
            self.status = format!("Cannot save the config: {e}");
            return;
        }
        self.status = "Saved. A running daemon applies the changes automatically.".into();
        match daemon_exe() {
            Some(exe) if exe.exists() => {
                if let Err(e) = autostart::set(self.cfg.common.autostart, &exe) {
                    self.status = format!("Saved, but autostart was not updated: {e}");
                }
            }
            _ if self.cfg.common.autostart => {
                self.status = "Saved, but lite-switcher.exe was not found next to this program (autostart skipped).".into();
            }
            _ => {}
        }
    }

    fn reload(&mut self) {
        match Config::load_from(&Config::path()) {
            Ok(cfg) => {
                self.cfg = cfg;
                self.load_form();
                self.status = "Reloaded from file.".into();
            }
            Err(e) => self.status = format!("Cannot read the config: {e}"),
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
        ui.heading("Keyboard layouts");
        ui.checkbox(&mut self.auto_layouts, "All system layouts (auto)");
        ui.label(if self.auto_layouts {
            "Uncheck the layouts that must not take part in the conversion cycle."
        } else {
            "Checked layouts take part in the cycle, in the order shown below."
        });
        let manual = !self.auto_layouts;
        let mut swap: Option<(usize, usize)> = None;
        let last = self.rows.len().saturating_sub(1);
        for (i, row) in self.rows.iter_mut().enumerate() {
            ui.horizontal(|ui| {
                ui.checkbox(&mut row.enabled, "");
                let mut label = format!("{}  {}", row.id, row.name);
                if !row.installed {
                    label.push_str("  (not installed)");
                }
                ui.label(label);
                if manual {
                    if ui.add_enabled(i > 0, egui::Button::new("Up")).clicked() {
                        swap = Some((i, i - 1));
                    }
                    if ui.add_enabled(i < last, egui::Button::new("Down")).clicked() {
                        swap = Some((i, i + 1));
                    }
                }
            });
        }
        if let Some((a, b)) = swap {
            self.rows.swap(a, b);
        }
        ui.separator();
        ui.heading("General");
        ui.checkbox(&mut self.cfg.common.autoswitch, "Автоперемикання");
        ui.label("Automatic layout switching is not implemented yet: the flag is only stored.");
        ui.checkbox(&mut self.cfg.common.autostart, "Start with Windows");
        ui.checkbox(&mut self.cfg.common.show_tray_icon, "Show the tray icon");
        ui.checkbox(&mut self.cfg.common.keep_trailing_space, "Keep spaces typed after the word when converting");
    }

    fn hotkeys_tab(&mut self, ui: &mut egui::Ui) {
        ui.heading("Hotkeys");
        for field in HotkeyField::ALL {
            ui.horizontal(|ui| {
                ui.label(format!("{}:", field.label()));
                if self.capturing == Some(field) {
                    ui.strong("press the new hotkey...");
                    if ui.button("Cancel").clicked() {
                        self.capturing = None;
                    }
                } else {
                    match self.hotkey(field) {
                        Some(hotkey) => ui.strong(hotkey.to_string()),
                        None => ui.weak("(not set)"),
                    };
                    if ui.button("Change...").clicked() {
                        self.capturing = Some(field);
                    }
                    if field.optional() && self.hotkey(field).is_some() && ui.button("Clear").clicked() {
                        self.set_hotkey(field, None);
                    }
                    if self.hotkey(field) != field.default_hotkey() && ui.button("Default").clicked() {
                        self.set_hotkey(field, field.default_hotkey());
                    }
                }
            });
        }
        ui.separator();
        ui.label("Pause and Break are the same key. Pressing the word hotkey again cycles through the other layouts and back to the original one.");
    }

    fn troubleshooting_tab(&mut self, ui: &mut egui::Ui) {
        ui.heading("Troubleshooting");
        let t = &mut self.cfg.troubleshooting;
        ui.checkbox(&mut t.log_enabled, "Write a log file (restart the daemon to apply)");
        ui.horizontal(|ui| {
            ui.label("Log level:");
            egui::ComboBox::from_id_salt("log_level").selected_text(t.log_level.clone()).show_ui(ui, |ui| {
                for level in ["error", "warn", "info", "debug"] {
                    ui.selectable_value(&mut t.log_level, level.to_string(), level);
                }
            });
        });
        ui.horizontal(|ui| {
            ui.label("Pause between synthetic keys (ms):");
            ui.add(egui::DragValue::new(&mut t.key_delay_ms).range(0..=100));
        });
        ui.label("Ignored applications (executable names, one per line, e.g. game.exe):");
        ui.add(egui::TextEdit::multiline(&mut self.ignored_text).desired_rows(4).desired_width(f32::INFINITY));
        ui.horizontal(|ui| {
            if ui.button("Open the config folder").clicked() {
                let _ = Command::new("explorer").arg(Config::dir()).spawn();
            }
        });
    }

    fn sounds_tab(&mut self, ui: &mut egui::Ui) {
        ui.heading("Sounds");
        ui.checkbox(&mut self.cfg.sounds.enabled, "Play a sound after the conversion");
        ui.label("Path to a .wav file (empty = the system default sound):");
        ui.add(egui::TextEdit::singleline(&mut self.cfg.sounds.on_convert).desired_width(f32::INFINITY));
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        if self.capturing.is_some() {
            let ctx = ui.ctx().clone();
            self.poll_hotkey_capture(&ctx);
        }
        egui::Panel::bottom("bottom").show(ui, |ui| {
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                if ui.button("Save").clicked() {
                    self.save();
                }
                if ui.button("Reload").clicked() {
                    self.reload();
                }
            });
            ui.label(&self.status);
            ui.add_space(4.0);
        });
        egui::CentralPanel::default().show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.selectable_value(&mut self.tab, Tab::Common, "Common");
                ui.selectable_value(&mut self.tab, Tab::Hotkeys, "Hotkeys");
                ui.selectable_value(&mut self.tab, Tab::Troubleshooting, "Troubleshooting");
                ui.selectable_value(&mut self.tab, Tab::Sounds, "Sounds");
            });
            ui.separator();
            egui::ScrollArea::vertical().show(ui, |ui| match self.tab {
                Tab::Common => self.common_tab(ui),
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
    eframe::run_native(
        "Lite Switcher - Settings",
        options,
        Box::new(|_cc| Ok(Box::new(App::new()))),
    )
}
