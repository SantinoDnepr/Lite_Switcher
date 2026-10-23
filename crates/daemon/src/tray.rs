//! Tray icon and its menu.

use std::path::PathBuf;
use std::process::Command;

use tray_icon::menu::{CheckMenuItem, Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use tray_icon::{Icon, TrayIcon, TrayIconBuilder};

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

pub struct Tray {
    icon: TrayIcon,
    autoswitch: CheckMenuItem,
}

impl Tray {
    pub fn new(autoswitch_on: bool) -> Result<Tray, Box<dyn std::error::Error>> {
        let autoswitch = CheckMenuItem::new("Автоперемикання", true, autoswitch_on, None);
        let settings = MenuItem::new("Settings...", true, None);
        let exit = MenuItem::new("Exit", true, None);
        let menu = Menu::new();
        menu.append_items(&[&autoswitch, &settings, &PredefinedMenuItem::separator(), &exit])?;

        let icon = TrayIconBuilder::new()
            .with_menu(Box::new(menu))
            .with_tooltip("Lite Switcher")
            .with_icon(make_icon())
            .build()?;

        // The handler must be Send + Sync, so it only touches ids and posts messages to the main thread.
        let (autoswitch_id, settings_id, exit_id) =
            (autoswitch.id().clone(), settings.id().clone(), exit.id().clone());
        MenuEvent::set_event_handler(Some(move |event: MenuEvent| {
            if event.id == autoswitch_id {
                post_action(Action::ToggleAutoswitch);
            } else if event.id == settings_id {
                if let Some(path) = settings_exe() {
                    if let Err(e) = Command::new(&path).spawn() {
                        tracing::warn!("cannot start {}: {e}", path.display());
                    }
                }
            } else if event.id == exit_id {
                post_quit();
            }
        }));
        Ok(Tray { icon, autoswitch })
    }

    pub fn set_visible(&self, visible: bool) {
        let _ = self.icon.set_visible(visible);
    }

    /// Syncs the check mark of the menu item with the autoswitch setting.
    pub fn refresh(&self, autoswitch_on: bool) {
        self.autoswitch.set_checked(autoswitch_on);
    }
}
