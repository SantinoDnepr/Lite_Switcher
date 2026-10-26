//! Folders with program data (translations, language packs) that live next to the executables.

use std::path::PathBuf;

/// For tests and development: when this environment variable is set, the data folders are looked
/// for only in the folder it names.
pub const DATA_DIR_VAR: &str = "LITE_SWITCHER_DATA_DIR";

/// Finds the folder `name` next to the executable or in one of its parent folders (so that it is
/// found in the project when the programs run from `target\debug`).
pub fn find_dir(name: &str) -> Option<PathBuf> {
    if let Some(base) = std::env::var_os(DATA_DIR_VAR) {
        return Some(PathBuf::from(base).join(name)).filter(|dir| dir.is_dir());
    }
    let exe = std::env::current_exe().ok()?;
    exe.parent()?.ancestors().take(5).map(|dir| dir.join(name)).find(|dir| dir.is_dir())
}
