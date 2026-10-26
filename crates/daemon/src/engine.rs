//! Language packs in the daemon: which packs are active, their opened data and the detectors.
//!
//! Opening the data can take a while, so it runs in a background thread; the finished set replaces
//! the previous one atomically. The keyboard hook only takes a reference to the current set.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;

use lite_switcher_core::config::Config;
use lite_switcher_core::layout::{self, key_to_char, Layout};
use lite_switcher_lang::literary::LiteraryDetector;
use lite_switcher_lang::programming::ProgrammingDetector;
use lite_switcher_lang::pack::{self, Loaded, PackStatus};
use lite_switcher_lang::render::RenderTable;

use crate::hook::{post_message, WM_PACKS_LOADED};

/// Everything the automatic switching needs; replaced as a whole when the packs are reloaded.
pub struct Engine {
    /// The opened data of the active packs.
    pub loaded: Loaded,
    /// The layouts of the cycle (the same as the layouts of the table, in the same order).
    pub layouts: Vec<Layout>,
    /// What every key types in every layout, and the class of every key.
    pub table: RenderTable,
    pub literary: LiteraryDetector,
    /// The detector of the selected programming language, if its pack is active.
    pub programming: Option<ProgrammingDetector>,
}

impl Engine {
    /// An engine that does nothing (before the first load, or without packs).
    fn empty() -> Engine {
        let loaded = Loaded::default();
        let table = RenderTable::build(Vec::new(), |_, _, _| None);
        Engine { literary: LiteraryDetector::new(&loaded, &[]), programming: None, loaded, layouts: Vec::new(), table }
    }

    /// Can the automatic literary mode decide anything?
    pub fn literary_ready(&self) -> bool {
        !self.layouts.is_empty() && (0..self.layouts.len()).any(|i| self.literary.serves(i))
    }

    /// Can the automatic programming mode decide anything?
    pub fn programming_ready(&self) -> bool {
        self.programming.as_ref().is_some_and(|p| (0..self.layouts.len()).any(|i| p.serves(i)))
    }
}

static GENERATION: AtomicU64 = AtomicU64::new(0);
static CURRENT: Mutex<Option<Arc<Engine>>> = Mutex::new(None);

/// The current engine (an empty one until the first load is finished).
pub fn current() -> Arc<Engine> {
    let current = CURRENT.lock().ok().and_then(|slot| slot.clone());
    current.unwrap_or_else(|| Arc::new(Engine::empty()))
}

fn describe(statuses: &[PackStatus]) -> String {
    let active: Vec<&str> =
        statuses.iter().filter(|s| s.active).map(|s| s.info.manifest.code.as_str()).collect();
    if active.is_empty() { "none".into() } else { active.join(", ") }
}

fn build(config: &Config) -> Engine {
    let found = pack::discover();
    let installed = layout::enumerate();
    let literary = pack::evaluate(&found.literary, &installed, config);
    let programming = pack::evaluate(&found.programming, &installed, config);
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

    // the table of the keys: ToUnicodeEx is asked once per key and layout, here, not while typing
    let layouts = layout::resolve_layouts(&installed, &config.common);
    let ids: Vec<String> = layouts.iter().map(|l| l.id.clone()).collect();
    let started = std::time::Instant::now();
    let table = RenderTable::build(ids, |i, scancode, shift| key_to_char(layouts[i].hkl, scancode, shift, false));
    tracing::debug!("key table for {} layouts built in {:?}", layouts.len(), started.elapsed());
    let detector = LiteraryDetector::new(&loaded, table.layout_ids());
    let programming = loaded.programming.clone().map(|pack| ProgrammingDetector::new(pack, &table));
    Engine { loaded, layouts, table, literary: detector, programming }
}

/// Finds the packs, decides which are active (config + installed layouts) and opens their data in
/// the background. Packs that stopped being active are dropped (their files are unmapped).
pub fn reload(config: &Config) {
    let config = config.clone();
    let generation = GENERATION.fetch_add(1, Ordering::SeqCst) + 1;
    thread::spawn(move || {
        let engine = build(&config);
        // a newer reload may have started meanwhile: only the latest result is used
        if GENERATION.load(Ordering::SeqCst) == generation {
            if let Ok(mut slot) = CURRENT.lock() {
                *slot = Some(Arc::new(engine));
            }
            post_message(WM_PACKS_LOADED);
        }
    });
}
