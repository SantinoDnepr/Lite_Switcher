//! Language packs in the daemon: which packs are active and their opened data.
//!
//! Opening the data can take a while, so it runs in a background thread; the finished set replaces
//! the previous one atomically. The keyboard hook only takes a reference to the current set.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;

use lite_switcher_core::config::Config;
use lite_switcher_core::layout;
use lite_switcher_lang::pack::{self, Loaded, PackStatus};

use crate::hook::{post_message, WM_PACKS_LOADED};

static GENERATION: AtomicU64 = AtomicU64::new(0);
static LOADED: Mutex<Option<Arc<Loaded>>> = Mutex::new(None);

/// The data of the active packs (empty until the first load is finished).
pub fn loaded() -> Arc<Loaded> {
    LOADED.lock().ok().and_then(|slot| slot.clone()).unwrap_or_default()
}

fn describe(statuses: &[PackStatus]) -> String {
    let active: Vec<&str> =
        statuses.iter().filter(|s| s.active).map(|s| s.info.manifest.code.as_str()).collect();
    if active.is_empty() { "none".into() } else { active.join(", ") }
}

/// Finds the packs, decides which are active (config + installed layouts) and opens their data in
/// the background. Packs that stopped being active are dropped (their files are unmapped).
pub fn reload(config: &Config) {
    let config = config.clone();
    let generation = GENERATION.fetch_add(1, Ordering::SeqCst) + 1;
    thread::spawn(move || {
        let found = pack::discover();
        let installed = layout::enumerate();
        let literary = pack::evaluate(&found.literary, &installed, &config);
        let programming = pack::evaluate(&found.programming, &installed, &config);
        let statuses: Vec<PackStatus> = literary.iter().chain(programming.iter()).cloned().collect();
        let (loaded, load_issues) = pack::load(&statuses);
        for issue in found.issues.iter().chain(load_issues.iter()) {
            tracing::warn!("language pack {}: {}", issue.dir.display(), issue.message);
        }
        tracing::info!(
            "language packs: found {} literary and {} programming; active literary: {}; programming: {}",
            found.literary.len(),
            found.programming.len(),
            describe(&literary),
            describe(&programming)
        );
        // a newer reload may have started meanwhile: only the latest result is used
        if GENERATION.load(Ordering::SeqCst) == generation {
            if let Ok(mut slot) = LOADED.lock() {
                *slot = Some(Arc::new(loaded));
            }
            post_message(WM_PACKS_LOADED);
        }
    });
}
