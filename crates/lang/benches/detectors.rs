//! Timing of the decisions (NFR-1, NFR-2, NFR-3). The packs are taken from the folders of the
//! repository; the programming benchmarks are skipped when the 1C pack has not been built.
//!
//! `cargo bench -p lite-switcher-lang`

use std::path::Path;
use std::sync::Arc;

use criterion::{black_box, criterion_group, criterion_main, Criterion};
use lite_switcher_lang::literary::{Context, LiteraryDetector};
use lite_switcher_lang::manifest::Kind;
use lite_switcher_lang::pack::{discover_dir, discover_in, load_pack, Loaded};
use lite_switcher_lang::programming::ProgrammingDetector;
use lite_switcher_lang::render::presets::{keys_for, table};
use lite_switcher_lang::render::KeyRenderer;

const ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../..");

fn literary(c: &mut Criterion) {
    let dir = Path::new(ROOT).join("LiteraryLanguages");
    let (infos, _) = discover_dir(&dir, Kind::Literary);
    let packs: Vec<_> = ["en", "ru", "uk"]
        .iter()
        .filter_map(|code| infos.iter().find(|i| i.manifest.code == *code))
        .filter_map(|i| load_pack(i).ok().map(Arc::new))
        .collect();
    if packs.len() < 3 {
        eprintln!("literary packs are not built: skipped");
        return;
    }
    let loaded = Loaded { literary: packs, programming: None };
    let render = table(&["en", "ru", "uk"]);
    let detector = LiteraryDetector::new(&loaded, render.layout_ids());

    let mut group = c.benchmark_group("literary");
    let cases = [
        ("wrong layout, 6 keys (ghbdtn)", keys_for("ghbdtn", "en"), 0),
        ("right word, 5 keys (hello)", keys_for("hello", "en"), 0),
        ("wrong layout, 12 keys", keys_for("ghjuhfvvbhjdfybt", "en")[..12].to_vec(), 0),
        ("long word, 24 keys", keys_for("internationalizationalism", "en")[..24].to_vec(), 0),
        ("long word, 40 keys", keys_for(&"internationalization".repeat(2), "en"), 0),
        ("ru word typed in ru", keys_for("привет", "ru"), 1),
    ];
    for (name, keys, current) in cases {
        group.bench_function(name, |b| {
            b.iter(|| detector.decide(&render, &Context { keys: black_box(&keys), current, previous: None }))
        });
    }
    group.finish();

    // NFR-2: the key -> character lookup that replaces ToUnicodeEx
    let keys = keys_for("ghbdtn", "en");
    c.bench_function("render: 6 keys through the table", |b| {
        b.iter(|| keys.iter().filter_map(|k| render.render(black_box(k), 1)).count())
    });
}

fn programming(c: &mut Criterion) {
    let dir = Path::new(ROOT).join("ProgrammingLanguages");
    let (infos, _) = discover_dir(&dir, Kind::Programming);
    let Some(pack) = infos.iter().find(|i| i.manifest.code == "1c").and_then(|i| load_pack(i).ok()) else {
        eprintln!("the 1c pack is not built: skipped");
        return;
    };
    let render = table(&["en", "ru"]);
    let detector = ProgrammingDetector::new(Arc::new(pack), &render);
    let mut group = c.benchmark_group("programming");
    let cases = [
        ("keyword typed in en (ghjwtlehf)", keys_for("ghjwtlehf", "en")),
        ("ordinary word (hello)", keys_for("hello", "en")),
        ("6 keys", keys_for("zghjc;", "en")),
        ("hybrid ЕстьNull in ru", keys_for("tcnmnull", "en")),
        ("24 keys", keys_for("ghjwtlehfbpvtybnmcjcnjzybt", "en")[..24].to_vec()),
        ("40 keys", keys_for(&"ghjwtlehf".repeat(5), "en")[..40].to_vec()),
    ];
    for (name, keys) in cases {
        group.bench_function(name, |b| {
            b.iter(|| detector.decide(&render, &Context { keys: black_box(&keys), current: 0, previous: None }))
        });
    }
    group.finish();
}

/// NFR-3: finding the packs (manifests only) and opening a pack.
fn packs(c: &mut Criterion) {
    let literary = Path::new(ROOT).join("LiteraryLanguages");
    let programming = Path::new(ROOT).join("ProgrammingLanguages");
    c.bench_function("discover the packs (manifests)", |b| {
        b.iter(|| discover_in(Some(black_box(&literary)), Some(black_box(&programming))))
    });
    let (infos, _) = discover_dir(&literary, Kind::Literary);
    if let Some(info) = infos.iter().find(|i| i.manifest.code == "ru") {
        c.bench_function("open the ru pack (checksums, dictionary, trigrams)", |b| b.iter(|| load_pack(black_box(info)).is_ok()));
    }
}

criterion_group!(benches, literary, programming, packs);
criterion_main!(benches);
