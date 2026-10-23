//! Start with Windows: `HKCU\Software\Microsoft\Windows\CurrentVersion\Run`.

use std::path::Path;

use windows_registry::CURRENT_USER;

const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
const VALUE_NAME: &str = "LiteSwitcher";

fn quoted(exe: &Path) -> String {
    format!("\"{}\"", exe.display())
}

/// True if autostart is registered for exactly this executable.
pub fn is_enabled(exe: &Path) -> bool {
    CURRENT_USER
        .open(RUN_KEY)
        .and_then(|k| k.get_string(VALUE_NAME))
        .map(|v| v == quoted(exe))
        .unwrap_or(false)
}

/// Adds (or removes) the daemon to the current user's autostart list.
pub fn set(enabled: bool, exe: &Path) -> Result<(), String> {
    if enabled {
        if is_enabled(exe) {
            return Ok(());
        }
        let key = CURRENT_USER.create(RUN_KEY).map_err(|e| e.to_string())?;
        key.set_string(VALUE_NAME, quoted(exe)).map_err(|e| e.to_string())
    } else {
        // Only the entry of this very executable is removed: another copy (or an older install
        // in a different folder) may own the entry.
        let Ok(key) = CURRENT_USER.create(RUN_KEY) else { return Ok(()) };
        match key.get_string(VALUE_NAME) {
            Ok(value) if value == quoted(exe) => key.remove_value(VALUE_NAME).map_err(|e| e.to_string()),
            _ => Ok(()),
        }
    }
}
