# Benchmarks

`cargo bench -p lite-switcher-lang` (criterion; the source is `crates/lang/benches/detectors.rs`).
The packs are taken from the repository folders; the 1C benchmarks run only when the 1C pack is built.
Measured on the development machine (Windows 10, release profile, other programs running, so the
numbers are an upper estimate). Time is the median per decision.

## Decisions (NFR-1)

Target: median up to 100 microseconds, a word of 40 keys up to 1 millisecond.

| Case | Literary | Programming (1C) |
|---|---|---|
| right word, 5 keys | 0.9 us | 12 us |
| wrong layout, 6 keys | 5.0 us | 19 us |
| 12 keys | 7.2 us | about 20-60 us |
| 24 keys | 9.1 us | 77 us |
| 40 keys | 12.9 us | 170 us |

The programming detector reads a token in every split of two layouts, so its time grows faster with the
length; a token of a program is rarely longer than 24 keys, and 40 keys stay far below the millisecond.
The keys of a token are rendered once per layout (one table lookup each) before the readings are built.

## Other measures

| What | Result | Target |
|---|---|---|
| key -> character through the table (6 keys) | 22 ns | NFR-2: a fraction of a microsecond (`ToUnicodeEx` costs 2.45 us per call) |
| finding the packs (manifests only) | 0.7 ms | NFR-3: up to 50 ms |
| opening the ru pack (checksums, dictionary, trigrams) | 2.6 ms | NFR-3: up to 200 ms |
| data of ru + uk + en + 1C pack | 1.5 MB of files | NFR-4: up to 10 MB more in the process |

The dictionaries are memory-mapped and the local lexicon is read into memory; a pack that is not active
is not opened at all.

## Accuracy

The accuracy measures of the detectors (false switches, restored words) are in
`docs/auto-switching-architecture.md` (6.3, 8) and are produced by `dictgen eval` and
`dictgen eval-programming`.
