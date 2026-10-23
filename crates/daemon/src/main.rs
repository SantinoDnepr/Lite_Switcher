#![windows_subsystem = "windows"]

mod clip;
mod hook;
mod inject;
mod selection;
mod tray;

use std::fs::{self, File, OpenOptions};
use std::sync::Mutex;
use std::time::SystemTime;

use windows::core::w;
use windows::Win32::Foundation::{GetLastError, ERROR_ALREADY_EXISTS};
use windows::Win32::System::Threading::CreateMutexW;
use windows::Win32::UI::WindowsAndMessaging::{
    DispatchMessageW, GetMessageW, SetTimer, TranslateMessage, MSG, WM_TIMER,
};

use lite_switcher_core::{autostart, config::Config, i18n::Translator};

const CONFIG_POLL_MS: u32 = 1000;

fn init_logging(config: &Config) {
    if !config.troubleshooting.log_enabled {
        return;
    }
    let level = match config.troubleshooting.log_level.as_str() {
        "error" => tracing::Level::ERROR,
        "warn" => tracing::Level::WARN,
        "debug" => tracing::Level::DEBUG,
        _ => tracing::Level::INFO,
    };
    let dir = Config::dir();
    let _ = fs::create_dir_all(&dir);
    let file: Option<File> = OpenOptions::new().create(true).append(true).open(dir.join("lite-switcher.log")).ok();
    if let Some(file) = file {
        let _ = tracing_subscriber::fmt()
            .with_writer(Mutex::new(file))
            .with_ansi(false)
            .with_max_level(level)
            .try_init();
    }
}

fn sync_autostart(config: &Config) {
    if let Ok(exe) = std::env::current_exe() {
        if let Err(e) = autostart::set(config.common.autostart, &exe) {
            tracing::warn!("autostart: {e}");
        }
    }
}

fn config_mtime() -> Option<SystemTime> {
    fs::metadata(Config::path()).and_then(|m| m.modified()).ok()
}

fn main() {
    // Single instance. LITE_SWITCHER_TEST_INSTANCE lets a test copy run next to the real daemon.
    let name = if std::env::var_os("LITE_SWITCHER_TEST_INSTANCE").is_some() {
        w!("Local\\LiteSwitcher.Daemon.Test")
    } else {
        w!("Local\\LiteSwitcher.Daemon")
    };
    let _mutex = unsafe {
        let handle = CreateMutexW(None, true, name);
        if handle.is_err() || GetLastError() == ERROR_ALREADY_EXISTS {
            return;
        }
        handle
    };

    let config = Config::load_or_create().unwrap_or_else(|_| Config::default());
    init_logging(&config);
    tracing::info!("lite-switcher started");
    sync_autostart(&config);
    hook::set_config(config.clone());

    let _hooks = match hook::install() {
        Ok(h) => h,
        Err(e) => {
            tracing::error!("cannot install keyboard hooks: {e}");
            return;
        }
    };
    let tray = match tray::Tray::new(config.common.autoswitch, &Translator::new(&config.common.language)) {
        Ok(t) => Some(t),
        Err(e) => {
            tracing::warn!("tray icon: {e}");
            None
        }
    };
    if let Some(t) = &tray {
        t.set_visible(config.common.show_tray_icon);
    }

    let timer = unsafe { SetTimer(None, 0, CONFIG_POLL_MS, None) };
    let mut last_mtime = config_mtime();

    let mut msg = MSG::default();
    unsafe {
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            if msg.message == hook::WM_ACTION {
                if let Some(action) = hook::Action::from_wparam(msg.wParam.0) {
                    if hook::handle_action(action) {
                        if let Some(t) = &tray {
                            t.refresh(hook::autoswitch_enabled());
                        }
                    }
                }
            } else if msg.message == WM_TIMER && msg.hwnd.0.is_null() && msg.wParam.0 == timer {
                let mtime = config_mtime();
                if mtime != last_mtime {
                    last_mtime = mtime;
                    match Config::load_from(&Config::path()) {
                        Ok(cfg) => {
                            tracing::info!("config reloaded");
                            sync_autostart(&cfg);
                            if let Some(t) = &tray {
                                t.set_visible(cfg.common.show_tray_icon);
                                t.refresh(cfg.common.autoswitch);
                                t.apply_language(&Translator::new(&cfg.common.language));
                            }
                            hook::set_config(cfg);
                        }
                        Err(e) => tracing::warn!("config reload failed, keeping the old one: {e}"),
                    }
                }
            } else {
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }
    }
    tracing::info!("lite-switcher stopped");
}
