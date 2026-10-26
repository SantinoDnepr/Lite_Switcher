//! `dictgen list`: prints the words of a pack (to see what is in it before it is published).

use std::path::PathBuf;

use lite_switcher_lang::format::{flags, frequency_class, Words};
use lite_switcher_lang::manifest::Manifest;

use crate::Args;

pub fn run(args: &Args) -> Result<(), String> {
    let dir = PathBuf::from(args.required("pack")?);
    let text = std::fs::read_to_string(dir.join("pack.ini")).map_err(|e| e.to_string())?;
    let manifest = Manifest::parse(&text, None).map_err(|e| e.to_string())?;
    let words = Words::open(&dir.join(&manifest.files.words)).map_err(|e| e.to_string())?;
    let limit: usize = args.number("limit", 50)?;
    let prefix = args.get("prefix").unwrap_or_default().to_lowercase();
    let wanted: u32 = args
        .get("flags")
        .unwrap_or_default()
        .split(',')
        .map(|f| match f.trim() {
            "keyword" => flags::KEYWORD,
            "directive" => flags::DIRECTIVE,
            "preprocessor" => flags::PREPROCESSOR,
            "platform" => flags::PLATFORM,
            "local" => flags::LOCAL,
            "hybrid" => flags::HYBRID,
            "no_convert" => flags::NO_CONVERT,
            _ => 0,
        })
        .fold(0, |a, b| a | b);
    let mut shown = 0;
    let total = words.len();
    for (word, value) in words.entries() {
        if !word.starts_with(&prefix) || (wanted != 0 && value & wanted == 0) {
            continue;
        }
        if shown < limit {
            println!("{word}\tfreq-class {}\tflags {:#x}", frequency_class_of(value), value >> 24);
        }
        shown += 1;
    }
    println!("{shown} of {total} words match");
    Ok(())
}

fn frequency_class_of(value: u32) -> u32 {
    let _ = frequency_class;
    value & 0xFFFF
}
