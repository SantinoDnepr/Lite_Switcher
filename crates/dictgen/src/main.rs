//! `dictgen`: builds the data of the language packs and measures how well they work.
//!
//! ```text
//! dictgen literary --code ru --name Russian --freq ru_full.txt --out LiteraryLanguages\ru
//!                  --license CC-BY-SA-4.0 --sources FrequencyWords-2018:ru [--min-count 30] ...
//! dictgen eval --packs LiteraryLanguages --freq en=en_full.txt,ru=ru_full.txt [--check]
//! ```

mod eval;
mod literary;

use std::collections::HashMap;
use std::process::ExitCode;

/// `--key value` pairs and `--flag`s after the command name.
pub struct Args {
    values: HashMap<String, String>,
}

impl Args {
    pub fn parse(args: &[String]) -> Result<Args, String> {
        let mut values = HashMap::new();
        let mut i = 0;
        while i < args.len() {
            let Some(key) = args[i].strip_prefix("--") else {
                return Err(format!("unexpected argument '{}'", args[i]));
            };
            match args.get(i + 1).filter(|v| !v.starts_with("--")) {
                Some(value) => {
                    values.insert(key.to_string(), value.clone());
                    i += 2;
                }
                None => {
                    values.insert(key.to_string(), String::new());
                    i += 1;
                }
            }
        }
        Ok(Args { values })
    }

    pub fn get(&self, key: &str) -> Option<&str> {
        self.values.get(key).map(String::as_str)
    }

    pub fn required(&self, key: &str) -> Result<&str, String> {
        self.get(key).filter(|v| !v.is_empty()).ok_or_else(|| format!("--{key} is required"))
    }

    pub fn flag(&self, key: &str) -> bool {
        self.values.contains_key(key)
    }

    pub fn number<T: std::str::FromStr>(&self, key: &str, default: T) -> Result<T, String> {
        match self.get(key) {
            None => Ok(default),
            Some(v) => v.parse().map_err(|_| format!("--{key} is not a number: '{v}'")),
        }
    }
}

const USAGE: &str = "\
usage:
  dictgen literary --code <code> --name <name> --freq <word-count list> --out <folder>
                   --license <license> --sources <sources> [--alphabet <letters>] [--match <prefix>]
                   [--min-count N] [--max-words N] [--min-len N] [--switch-margin N] [--dict-margin N] [--max-cost N]
                   [--forbidden <bigrams,...>] [--unique <letters>] [--exceptions <file>] [--fold-yo]
  dictgen eval --packs <LiteraryLanguages folder> --freq <code=file,code=file,...>
               [--sample N] [--head-rank N] [--switch-margin N] [--dict-margin N] [--max-cost N] [--check]";

fn run() -> Result<(), String> {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    let Some(command) = argv.first() else { return Err(USAGE.into()) };
    let args = Args::parse(&argv[1..])?;
    match command.as_str() {
        "literary" => literary::run(&args),
        "eval" => eval::run(&args),
        other => Err(format!("unknown command '{other}'\n{USAGE}")),
    }
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("{message}");
            ExitCode::FAILURE
        }
    }
}
