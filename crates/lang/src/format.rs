//! Data files of a pack: `words.fst`, `ngram3.bin`, `exceptions.txt` (see the specification, section 6).

use std::collections::HashSet;
use std::fmt;
use std::fs::{self, File};
use std::path::Path;

use fst::{Map, MapBuilder};
use memmap2::Mmap;
use sha2::{Digest, Sha256};

/// Flags in the bits 24-31 of a `words.fst` value; the bits 0-15 are the frequency class.
pub mod flags {
    pub const KEYWORD: u32 = 1 << 24;
    pub const DIRECTIVE: u32 = 1 << 25;
    pub const PREPROCESSOR: u32 = 1 << 26;
    pub const PLATFORM: u32 = 1 << 27;
    pub const LOCAL: u32 = 1 << 28;
    pub const HYBRID: u32 = 1 << 29;
    pub const NO_CONVERT: u32 = 1 << 30;
    pub const MASK: u32 = 0xFF00_0000;
}

pub fn frequency_class(value: u32) -> u16 {
    (value & 0xFFFF) as u16
}

pub fn flags_of(value: u32) -> u32 {
    value & flags::MASK
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FormatError(pub String);

impl fmt::Display for FormatError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for FormatError {}

fn ferr<T>(message: impl Into<String>) -> Result<T, FormatError> {
    Err(FormatError(message.into()))
}

/// The bytes of a data file: mapped from the disk or owned (tests, small files).
pub enum Backing {
    Mapped(Mmap),
    Owned(Vec<u8>),
}

impl AsRef<[u8]> for Backing {
    fn as_ref(&self) -> &[u8] {
        match self {
            Backing::Mapped(m) => m,
            Backing::Owned(v) => v,
        }
    }
}

/// SHA-256 of the data as lower case hex.
pub fn sha256_hex(data: &[u8]) -> String {
    Sha256::digest(data).iter().map(|b| format!("{b:02x}")).collect()
}

/// The words of a pack: lower case key -> `u32` (frequency class and flags).
pub struct Words {
    map: Map<Backing>,
}

impl Words {
    /// Maps the file into memory (only the touched pages are read from the disk).
    pub fn open(path: &Path) -> Result<Words, FormatError> {
        let file = File::open(path).map_err(|e| FormatError(format!("{}: {e}", path.display())))?;
        let len = file.metadata().map_err(|e| FormatError(e.to_string()))?.len();
        if len == 0 {
            return ferr(format!("{} is empty", path.display()));
        }
        // The file is only read; it is not expected to change while the program runs.
        let mmap = unsafe { Mmap::map(&file) }.map_err(|e| FormatError(format!("{}: {e}", path.display())))?;
        Words::from_backing(Backing::Mapped(mmap))
    }

    pub fn from_bytes(bytes: Vec<u8>) -> Result<Words, FormatError> {
        Words::from_backing(Backing::Owned(bytes))
    }

    fn from_backing(backing: Backing) -> Result<Words, FormatError> {
        Map::new(backing).map(|map| Words { map }).map_err(|e| FormatError(format!("not a valid words.fst: {e}")))
    }

    /// Serializes entries. Keys are lower-cased by the caller. Duplicates are merged: the flags are
    /// combined, the larger frequency class wins.
    pub fn build(entries: impl IntoIterator<Item = (String, u32)>) -> Result<Vec<u8>, FormatError> {
        let mut entries: Vec<(String, u32)> = entries.into_iter().collect();
        entries.sort_by(|a, b| a.0.as_bytes().cmp(b.0.as_bytes()));
        let mut merged: Vec<(String, u32)> = Vec::with_capacity(entries.len());
        for (key, value) in entries {
            match merged.last_mut() {
                Some((last, v)) if *last == key => {
                    *v = flags_of(*v) | flags_of(value) | u32::from(frequency_class(*v).max(frequency_class(value)));
                }
                _ => merged.push((key, value)),
            }
        }
        let mut builder = MapBuilder::memory();
        for (key, value) in &merged {
            builder.insert(key.as_bytes(), u64::from(*value)).map_err(|e| FormatError(e.to_string()))?;
        }
        builder.into_inner().map_err(|e| FormatError(e.to_string()))
    }

    pub fn get(&self, key: &str) -> Option<u32> {
        self.map.get(key.as_bytes()).map(|v| v as u32)
    }

    pub fn contains(&self, key: &str) -> bool {
        self.map.contains_key(key.as_bytes())
    }

    pub fn len(&self) -> usize {
        self.map.len()
    }

    /// All entries in the order of the keys (for the tools; the program never lists the dictionary).
    pub fn entries(&self) -> Vec<(String, u32)> {
        use fst::{IntoStreamer, Streamer};
        let mut stream = self.map.stream().into_stream();
        let mut out = Vec::with_capacity(self.map.len());
        while let Some((key, value)) = stream.next() {
            out.push((String::from_utf8_lossy(key).into_owned(), value as u32));
        }
        out
    }

    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }
}

const NGRAM_MAGIC: &[u8; 4] = b"LSNG";
/// Cost value that means "impossible".
pub const NGRAM_IMPOSSIBLE: u8 = 255;

/// Letter trigram costs: `u8` per trigram, `-log2 p * scale`, 255 = impossible. Index 0 of every
/// position is the word boundary, the letters of the alphabet are 1..=A.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ngram {
    alphabet: Vec<char>,
    table: Vec<u8>,
}

impl Ngram {
    pub fn new(alphabet: Vec<char>, table: Vec<u8>) -> Result<Ngram, FormatError> {
        let side = alphabet.len() + 1;
        if alphabet.is_empty() || alphabet.len() > u16::MAX as usize - 1 {
            return ferr("the alphabet is empty or too large");
        }
        if table.len() != side * side * side {
            return ferr(format!("the table has {} entries, {} expected", table.len(), side * side * side));
        }
        Ok(Ngram { alphabet, table })
    }

    pub fn alphabet(&self) -> &[char] {
        &self.alphabet
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        let alphabet: String = self.alphabet.iter().collect();
        let mut out = Vec::with_capacity(16 + alphabet.len() + self.table.len());
        out.extend_from_slice(NGRAM_MAGIC);
        out.extend_from_slice(&1u16.to_le_bytes()); // format version
        out.extend_from_slice(&(self.alphabet.len() as u16).to_le_bytes());
        out.extend_from_slice(&(self.table.len() as u32).to_le_bytes());
        out.extend_from_slice(&(alphabet.len() as u16).to_le_bytes());
        out.extend_from_slice(alphabet.as_bytes());
        out.extend_from_slice(&self.table);
        out
    }

    pub fn parse(bytes: &[u8]) -> Result<Ngram, FormatError> {
        let header = |from: usize, len: usize| bytes.get(from..from + len).ok_or_else(|| FormatError("ngram file is truncated".into()));
        if header(0, 4)? != NGRAM_MAGIC {
            return ferr("not an ngram file (bad magic)");
        }
        let u16_at = |at: usize| header(at, 2).map(|b| u16::from_le_bytes([b[0], b[1]]) as usize);
        let version = u16_at(4)?;
        if version != 1 {
            return ferr(format!("ngram format {version} is not supported"));
        }
        let letters = u16_at(6)?;
        let table_len = header(8, 4).map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]) as usize)?;
        let alphabet_bytes = u16_at(12)?;
        let alphabet = std::str::from_utf8(header(14, alphabet_bytes)?).map_err(|_| FormatError("the alphabet is not UTF-8".into()))?;
        let alphabet: Vec<char> = alphabet.chars().collect();
        if alphabet.len() != letters {
            return ferr("the alphabet length does not match the header");
        }
        let table = header(14 + alphabet_bytes, table_len)?.to_vec();
        Ngram::new(alphabet, table)
    }

    pub fn open(path: &Path) -> Result<Ngram, FormatError> {
        Ngram::parse(&fs::read(path).map_err(|e| FormatError(format!("{}: {e}", path.display())))?)
    }

    fn index_of(&self, c: char) -> Option<usize> {
        self.alphabet.iter().position(|a| *a == c).map(|i| i + 1)
    }

    /// The cost of a word: the sum over its trigrams including the boundaries (lower is more
    /// likely). `None` if the word has a letter outside the alphabet or an impossible trigram.
    pub fn cost(&self, word: &str) -> Option<u32> {
        let side = self.alphabet.len() + 1;
        let mut idx = vec![0usize, 0usize];
        for c in word.chars() {
            idx.push(self.index_of(c)?);
        }
        idx.push(0);
        let mut total = 0u32;
        for w in idx.windows(3) {
            let v = self.table[(w[0] * side + w[1]) * side + w[2]];
            if v == NGRAM_IMPOSSIBLE {
                return None;
            }
            total += u32::from(v);
        }
        Some(total)
    }
}

/// `exceptions.txt`: one word per line, `;` or `#` start a comment; the words are lower-cased.
pub fn parse_exceptions(text: &str) -> HashSet<String> {
    text.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with(';') && !l.starts_with('#'))
        .map(str::to_lowercase)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn words_roundtrip_and_merge() {
        let bytes = Words::build([
            ("привет".to_string(), 100),
            ("процедура".to_string(), flags::KEYWORD | 7),
            ("процедура".to_string(), flags::HYBRID | 9),
            ("#область".to_string(), flags::PREPROCESSOR),
        ])
        .unwrap();
        let words = Words::from_bytes(bytes).unwrap();
        assert_eq!(words.len(), 3);
        assert!(words.contains("привет") && !words.contains("Привет") && !words.contains("пока"));
        let v = words.get("процедура").unwrap();
        assert_eq!(flags_of(v), flags::KEYWORD | flags::HYBRID);
        assert_eq!(frequency_class(v), 9);
        assert_eq!(words.get("#область"), Some(flags::PREPROCESSOR));
    }

    #[test]
    fn words_open_maps_a_file_and_rejects_garbage() {
        let dir = std::env::temp_dir().join(format!("ls-lang-words-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let good = dir.join("words.fst");
        fs::write(&good, Words::build([("a".to_string(), 1)]).unwrap()).unwrap();
        assert!(Words::open(&good).unwrap().contains("a"));
        let empty = dir.join("empty.fst");
        fs::write(&empty, b"").unwrap();
        assert!(Words::open(&empty).is_err());
        let garbage = dir.join("garbage.fst");
        fs::write(&garbage, b"this is not an fst at all").unwrap();
        assert!(Words::open(&garbage).is_err());
        assert!(Words::open(&dir.join("missing.fst")).is_err());
        fs::remove_dir_all(&dir).unwrap();
    }

    fn tiny_ngram() -> Ngram {
        // alphabet "ab": side 3, table 27 entries; every trigram costs 10, "bb?" is impossible
        let mut table = vec![10u8; 27];
        for third in 0..3 {
            table[(2 * 3 + 2) * 3 + third] = NGRAM_IMPOSSIBLE;
        }
        Ngram::new(vec!['a', 'b'], table).unwrap()
    }

    #[test]
    fn ngram_roundtrip_and_cost() {
        let n = tiny_ngram();
        assert_eq!(Ngram::parse(&n.to_bytes()).unwrap(), n);
        assert_eq!(n.cost("ab"), Some(30), "three trigrams: ^^a, ^ab, ab^ (the word length + 1)");
        assert_eq!(n.cost("abba"), None, "'bb' is impossible");
        assert_eq!(n.cost("abc"), None, "'c' is not in the alphabet");
    }

    #[test]
    fn ngram_rejects_bad_files() {
        let good = tiny_ngram().to_bytes();
        assert!(Ngram::parse(&good[..10]).is_err(), "truncated");
        let mut bad_magic = good.clone();
        bad_magic[0] = b'X';
        assert!(Ngram::parse(&bad_magic).is_err());
        let mut bad_version = good.clone();
        bad_version[4] = 9;
        assert!(Ngram::parse(&bad_version).is_err());
        assert!(Ngram::new(vec!['a'], vec![0; 5]).is_err(), "wrong table size");
        assert!(Ngram::new(vec![], vec![0]).is_err());
    }

    #[test]
    fn exceptions_skip_comments() {
        let set = parse_exceptions("; comment\n# another\n\nНАТО \n  Ikea\n");
        assert_eq!(set.len(), 2);
        assert!(set.contains("нато") && set.contains("ikea"));
    }

    #[test]
    fn sha256_matches_the_known_value() {
        assert_eq!(sha256_hex(b"abc"), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
    }
}
