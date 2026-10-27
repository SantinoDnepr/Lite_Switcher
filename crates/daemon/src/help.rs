//! The built-in help: the pages `Help\Help_<language code>.html` next to the program.

use std::path::{Path, PathBuf};

use lite_switcher_core::paths;

const HELP_DIR: &str = "Help";
const FALLBACK: &str = "en";

fn page_name(code: &str) -> String {
    format!("Help_{code}.html")
}

/// The page for the language `code` in `dir`; the English page if there is none.
fn page_in(dir: &Path, code: &str) -> Option<PathBuf> {
    let code = code.trim().to_ascii_lowercase();
    // the code is part of a file name: letters, digits and `-` only
    let safe = !code.is_empty() && code.chars().all(|c| c.is_ascii_alphanumeric() || c == '-');
    let mut codes = Vec::new();
    if safe {
        codes.push(code);
    }
    codes.push(FALLBACK.to_string());
    codes.iter().map(|c| dir.join(page_name(c))).find(|p| p.is_file())
}

pub fn find_page(code: &str) -> Option<PathBuf> {
    page_in(&paths::find_dir(HELP_DIR)?, code)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn the_page_of_the_language_or_english() {
        let dir = std::env::temp_dir().join(format!("ls-help-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("Help_en.html"), "en").unwrap();
        fs::write(dir.join("Help_ru.html"), "ru").unwrap();
        assert_eq!(page_in(&dir, "ru"), Some(dir.join("Help_ru.html")));
        assert_eq!(page_in(&dir, "RU"), Some(dir.join("Help_ru.html")));
        assert_eq!(page_in(&dir, "uk"), Some(dir.join("Help_en.html")), "no page: English");
        assert_eq!(page_in(&dir, "../x"), Some(dir.join("Help_en.html")), "a code is not a path");
        assert_eq!(page_in(&dir, ""), Some(dir.join("Help_en.html")));
        fs::remove_file(dir.join("Help_en.html")).unwrap();
        assert_eq!(page_in(&dir, "uk"), None);
        fs::remove_dir_all(&dir).unwrap();
    }

    /// Every language of the `Language` folder (and English) has a help page in the repository.
    #[test]
    fn every_language_has_a_help_page() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let mut codes = vec!["en".to_string()];
        for entry in fs::read_dir(root.join("Language")).unwrap().flatten() {
            let text = fs::read_to_string(entry.path()).unwrap();
            let code = text.lines().find_map(|l| l.trim().strip_prefix("code =")).map(|c| c.trim().to_string());
            codes.extend(code);
        }
        codes.sort();
        codes.dedup();
        assert!(codes.len() >= 3);
        for code in codes {
            let page = root.join(HELP_DIR).join(page_name(&code));
            let html = fs::read_to_string(&page).unwrap_or_else(|_| panic!("{} is missing", page.display()));
            assert!(html.contains(&format!("<html lang=\"{code}\"")), "{}: the lang attribute", page.display());
            assert!(html.contains("</html>") && html.len() > 4000, "{}: looks unfinished", page.display());
        }
    }
}
