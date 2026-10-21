//! Tray icon and its menu.

use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::Ordering;

use tray_icon::menu::{CheckMenuItem, Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use tray_icon::{Icon, TrayIcon, TrayIconBuilder};
use windows::Win32::UI::WindowsAndMessaging::WM_QUIT;

use crate::hook::{post_to_main, PAUSED, WM_TRAY_REFRESH};

/// A round icon with the letter "L"; gray while paused.
fn make_icon(active: bool) -> Icon {
    const N: u32 = 32;
    let (r, g, b) = if active { (45, 127, 249) } else { (130, 130, 130) };
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
    pause: CheckMenuItem,
}

impl Tray {
    pub fn new() -> Result<Tray, Box<dyn std::error::Error>> {
        let settings = MenuItem::new("Settings...", true, None);
        let pause = CheckMenuItem::new("Pause", true, false, None);
        let exit = MenuItem::new("Exit", true, None);
        let menu = Menu::new();
        menu.append_items(&[&settings, &pause, &PredefinedMenuItem::separator(), &exit])?;

        let icon = TrayIconBuilder::new()
            .with_menu(Box::new(menu))
            .with_tooltip("Lite Switcher")
            .with_icon(make_icon(true))
            .build()?;

        // The handler must be Send + Sync, so it only touches ids and posts messages to the main thread.
        let (settings_id, pause_id, exit_id) = (settings.id().clone(), pause.id().clone(), exit.id().clone());
        MenuEvent::set_event_handler(Some(move |event: MenuEvent| {
            if event.id == settings_id {
                if let Some(path) = settings_exe() {
                    if let Err(e) = Command::new(&path).spawn() {
                        tracing::warn!("cannot start {}: {e}", path.display());
                    }
                }
            } else if event.id == pause_id {
                PAUSED.fetch_xor(true, Ordering::Relaxed);
                post_to_main(WM_TRAY_REFRESH);
            } else if event.id == exit_id {
                post_to_main(WM_QUIT);
            }
        }));
        Ok(Tray { icon, pause })
    }

    pub fn set_visible(&self, visible: bool) {
        let _ = self.icon.set_visible(visible);
    }

    /// Syncs the icon and the menu check mark with the pause state.
    pub fn refresh(&self) {
        let paused = PAUSED.load(Ordering::Relaxed);
        self.pause.set_checked(paused);
        let _ = self.icon.set_icon(Some(make_icon(!paused)));
        tracing::info!("paused = {paused}");
    }
}
