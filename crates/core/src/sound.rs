//! The sound played after a conversion: finding the file and playing it.
//!
//! The setting is either a bare file name (`switch2.wav`), which is looked for in the `sounds` folder
//! of the program, or a full path. An empty setting means the system sound.

use std::path::{Path, PathBuf};

use windows::core::{w, PCWSTR};
use windows::Win32::Media::Audio::{PlaySoundW, SND_ALIAS, SND_ASYNC, SND_FILENAME, SND_NODEFAULT};

/// The folder (next to the program) with the bundled sounds.
pub const SOUNDS_DIR: &str = "sounds";

/// The folder `sounds` of the program, if there is one.
pub fn sounds_dir() -> Option<PathBuf> {
    crate::paths::find_dir(SOUNDS_DIR)
}

/// The file the setting points to. A full path stays as it is. A relative name is looked for in the
/// `sounds` folder of the program, and then relative to the program itself (an old setting such as
/// `sounds\switch1.wav`). A name that is not found is returned unchanged.
pub fn resolve(wav: &str) -> PathBuf {
    let path = Path::new(wav);
    if wav.is_empty() || path.is_absolute() {
        return path.to_path_buf();
    }
    if let Some(found) = sounds_dir().map(|dir| dir.join(path)).filter(|p| p.is_file()) {
        return found;
    }
    let beside = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().and_then(|dir| dir.ancestors().take(crate::paths::SEARCH_DEPTH).map(|d| d.join(path)).find(|p| p.is_file())));
    beside.unwrap_or_else(|| path.to_path_buf())
}

/// What to put in the setting for a chosen file: just the name when the file is in the `sounds`
/// folder of the program, the full path otherwise.
pub fn setting_for(file: &Path) -> String {
    if let (Some(dir), Some(name)) = (sounds_dir(), file.file_name()) {
        let same_dir = file.parent().and_then(|p| p.canonicalize().ok()) == dir.canonicalize().ok();
        if same_dir {
            return name.to_string_lossy().into_owned();
        }
    }
    file.display().to_string()
}

/// Plays the sound of the setting (asynchronously): the system sound when it is empty.
pub fn play(wav: &str) {
    unsafe {
        if wav.is_empty() {
            let _ = PlaySoundW(w!("SystemAsterisk"), None, SND_ALIAS | SND_ASYNC);
        } else {
            let wide: Vec<u16> = resolve(wav).display().to_string().encode_utf16().chain(std::iter::once(0)).collect();
            let _ = PlaySoundW(PCWSTR(wide.as_ptr()), None, SND_FILENAME | SND_ASYNC | SND_NODEFAULT);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_bare_name_is_looked_for_in_the_sounds_folder() {
        // the test program runs in target: the `sounds` folder of the repository is above it
        let found = resolve("switch1.wav");
        assert!(
            found.is_absolute() && found.ends_with(r"sounds\switch1.wav"),
            "{found:?}, exe {:?}, sounds {:?}",
            std::env::current_exe(),
            sounds_dir()
        );
        // an old setting with the folder in it still works
        let old = resolve(r"sounds\switch1.wav");
        assert!(old.is_absolute() && old.ends_with("switch1.wav"), "{old:?}");
    }

    #[test]
    fn a_full_path_and_an_unknown_name_stay_as_they_are() {
        assert_eq!(resolve(r"C:\Windows\Media\tada.wav"), PathBuf::from(r"C:\Windows\Media\tada.wav"));
        assert_eq!(resolve("no-such.wav"), PathBuf::from("no-such.wav"));
        assert_eq!(resolve(""), PathBuf::new());
    }

    #[test]
    fn a_file_from_the_sounds_folder_is_stored_by_name_only() {
        let in_folder = resolve("switch2.wav");
        assert_eq!(setting_for(&in_folder), "switch2.wav");
        assert_eq!(setting_for(Path::new(r"C:\Windows\Media\tada.wav")), r"C:\Windows\Media\tada.wav");
    }
}
