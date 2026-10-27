//! Tray icon and its menu.

use std::path::PathBuf;
use std::process::Command;

use tray_icon::menu::{CheckMenuItem, Menu, MenuEvent, MenuId, MenuItem, PredefinedMenuItem, Submenu};
use tray_icon::{Icon, TrayIcon, TrayIconBuilder};

use lite_switcher_core::config::Mode;
use lite_switcher_core::i18n::Translator;

use crate::hook::{post_action, post_quit, Action};

/// A round icon with the letter "L".
fn make_icon() -> Icon {
    const N: u32 = 32;
    let (r, g, b) = (45, 127, 249);
    let mut rgba = Vec::with_capacity((N * N * 4) as usize);
    for y in 0..N {
        for x in 0..N {
            let (dx, dy) = (x as f32 - 15.5, y as f32 - 15.5);
            let inside = dx * dx + dy * dy <= 15.0 * 15.0;
            let letter = ((10..14).contains(&x) && (8..24).contains(&y))
                || ((10..22).contains(&x) && (20..24).contains(&y));
            let px = match (inside, letter) {
                (true, true) => [255, 255, 255, 255],
                (true, false) => [r, g, b, 255],
                _ => [0, 0, 0, 0],
            };
            rgba.extend_from_slice(&px);
        }
    }
    Icon::from_rgba(rgba, N, N).expect("valid icon data")
}

fn settings_exe() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    Some(exe.with_file_name("lite-switcher-settings.exe"))
}

/// The modes in the order of the "Mode" submenu.
const MENU_MODES: [Mode; 3] = [Mode::AutoLiterary, Mode::AutoProgramming, Mode::Manual];

fn mode_text_key(mode: Mode) -> &'static str {
    match mode {
        Mode::AutoLiterary => "tray.mode_auto_literary",
        Mode::AutoProgramming => "tray.mode_auto_programming",
        Mode::Manual => "tray.mode_manual",
    }
}

pub struct Tray {
    icon: TrayIcon,
    mode_menu: Submenu,
    /// Check items of the submenu, in the order of `MENU_MODES`.
    mode_items: Vec<CheckMenuItem>,
    settings: MenuItem,
    help: MenuItem,
    about: MenuItem,
    exit: MenuItem,
}

impl Tray {
    pub fn new(mode: Mode, tr: &Translator) -> Result<Tray, Box<dyn std::error::Error>> {
        // The Windows menu has no radio items: the check marks are kept exclusive by `refresh`.
        let mode_menu = Submenu::new(tr.t("tray.mode"), true);
        let mode_items: Vec<CheckMenuItem> =
            MENU_MODES.iter().map(|m| CheckMenuItem::new(tr.t(mode_text_key(*m)), true, *m == mode, None)).collect();
        for item in &mode_items {
            mode_menu.append(item)?;
        }
        let settings = MenuItem::new(tr.t("tray.settings"), true, None);
        let help = MenuItem::new(tr.t("tray.help"), true, None);
        let about = MenuItem::new(tr.t("tray.about"), true, None);
        let exit = MenuItem::new(tr.t("tray.exit"), true, None);
        let menu = Menu::new();
        menu.append_items(&[
            &mode_menu,
            &settings,
            &PredefinedMenuItem::separator(),
            &help,
            &about,
            &PredefinedMenuItem::separator(),
            &exit,
        ])?;

        let icon = TrayIconBuilder::new()
            .with_menu(Box::new(menu))
            .with_tooltip(tr.t("tray.tooltip"))
            .with_icon(make_icon())
            .build()?;

        // The handler must be Send + Sync, so it only touches ids and posts messages to the main thread.
        let mode_ids: Vec<(MenuId, Mode)> =
            mode_items.iter().zip(MENU_MODES).map(|(item, m)| (item.id().clone(), m)).collect();
        let (settings_id, help_id, about_id, exit_id) =
            (settings.id().clone(), help.id().clone(), about.id().clone(), exit.id().clone());
        MenuEvent::set_event_handler(Some(move |event: MenuEvent| {
            if let Some((_, mode)) = mode_ids.iter().find(|(id, _)| *id == event.id) {
                post_action(Action::set_mode(*mode));
            } else if event.id == settings_id {
                if let Some(path) = settings_exe() {
                    if let Err(e) = Command::new(&path).spawn() {
                        tracing::warn!("cannot start {}: {e}", path.display());
                    }
                }
            } else if event.id == help_id {
                // the language of the help is the language of the program: the main thread knows it
                post_action(Action::ShowHelp);
            } else if event.id == about_id {
                if let Some(path) = settings_exe() {
                    if let Err(e) = Command::new(&path).arg("--about").spawn() {
                        tracing::warn!("cannot start {}: {e}", path.display());
                    }
                }
            } else if event.id == exit_id {
                post_quit();
            }
        }));
        Ok(Tray { icon, mode_menu, mode_items, settings, help, about, exit })
    }

    pub fn set_visible(&self, visible: bool) {
        let _ = self.icon.set_visible(visible);
    }

    /// Shows the texts of the menu and the tooltip in the given language.
    pub fn apply_language(&self, tr: &Translator) {
        self.mode_menu.set_text(tr.t("tray.mode"));
        for (item, mode) in self.mode_items.iter().zip(MENU_MODES) {
            item.set_text(tr.t(mode_text_key(mode)));
        }
        self.settings.set_text(tr.t("tray.settings"));
        self.help.set_text(tr.t("tray.help"));
        self.about.set_text(tr.t("tray.about"));
        self.exit.set_text(tr.t("tray.exit"));
        let _ = self.icon.set_tooltip(Some(tr.t("tray.tooltip")));
    }

    /// Tooltip of the icon; with `no_packs` it also says that the selected automatic mode has no
    /// active language pack and works as manual.
    pub fn set_hint(&self, tr: &Translator, no_packs: bool) {
        let tooltip = if no_packs {
            format!("{}\n{}", tr.t("tray.tooltip"), tr.t("common.mode_hint_no_packs"))
        } else {
            tr.t("tray.tooltip").to_string()
        };
        let _ = self.icon.set_tooltip(Some(tooltip));
    }

    /// Puts the check mark on the item of the current mode (and only on it).
    pub fn refresh(&self, mode: Mode) {
        for (item, m) in self.mode_items.iter().zip(MENU_MODES) {
            item.set_checked(m == mode);
        }
    }
}
