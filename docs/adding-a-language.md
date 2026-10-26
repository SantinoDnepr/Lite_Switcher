# How to add a language

A language is a folder with a `pack.ini` and data files. No code has to change: put the folder next to
the executables and restart the daemon (or change a setting; the packs are read again when the config
changes). The folder is searched next to the exe and up to five levels above it, or in the folder of
the environment variable `LITE_SWITCHER_DATA_DIR`.

| Kind | Folder | Active when |
|---|---|---|
| Literary (a human language) | `LiteraryLanguages\<code>\` | it is checked in Settings and a keyboard layout of the language is installed |
| Programming | `ProgrammingLanguages\<id>\` | it is selected (one at a time) and all layouts it needs are installed |

The tool that builds the data is `dictgen` (`cargo build --release -p lite-switcher-dictgen`). It writes
`pack.ini` with the checksums of the files, so a pack that is changed by hand is rejected at start.
A pack that cannot be opened is skipped and reported in the log; it never stops the daemon.

## A literary language

You need a list of words with their counts (`word count` per line, for example a frequency list of
a subtitles or news corpus). Check the licence of the data first: the pack gets the licence of its
sources. Data under a non-commercial or an unknown licence must not be used for a published pack.

```
dictgen literary --code de --name German --freq de_50k.txt --out LiteraryLanguages\de ^
  --license CC-BY-SA-4.0 --sources "FrequencyWords 2018 (de)" ^
  --alphabet "abcdefghijklmnopqrstuvwxyzäöüß" --match de --min-count 150
```

- `--match` is the beginning of the locale name of the keyboard layout (`de` matches `de-DE`, `de-AT`).
- `--min-count` throws out rare words: the dictionary must hold words that people really type.
  Pick the value with `dictgen eval` (false switches on frequent words have to stay near 0%).
- `--forbidden` (letter pairs that never occur in the language), `--unique` (letters that exist only in
  this language) and `--exceptions` (words that are never changed) improve the result.
- The result: `pack.ini`, `words.fst` (the dictionary), `ngram3.bin` (letter triples), `exceptions.txt`.

Measure the result before you ship it:

```
dictgen eval --packs LiteraryLanguages --freq de=de_50k.txt,en=en_50k.txt,ru=ru_50k.txt --check
```

## A programming language (Rust, C++, ...)

A programming pack is a dictionary of the tokens of the language: keywords, directives, names of the
standard library. The program recognises a token that was typed in the wrong layout (for example
`ыев` typed in the Russian layout instead of `std`) and replaces it by the token of the dictionary.

1. Write a seed list, one entry per line: `<form in the native layout>|<form in the Latin layout>|<flags>`;
   an empty form is allowed. The flags: `keyword`, `directive`, `preprocessor`, `platform`, `hybrid`
   (a token that is written in two layouts, like `ЕстьNull`), `no_convert`. Lines that start with `;`
   are comments. A token of up to three letters is replaced only when it has the flag `keyword`,
   `directive` or `preprocessor` (short names are also ordinary words of other languages).

   ```
   ; ProgrammingLanguages\rust\seed.txt
   fn||keyword
   impl||keyword
   struct||keyword
   trait||keyword
   ```

2. Build the pack:

   ```
   dictgen programming --code rust --name Rust --seed ProgrammingLanguages\rust\seed.txt ^
     --out ProgrammingLanguages\rust --license MIT --sources "language reference" ^
     --require en --roles "en:native" --sigils "#" --latin-segments ""
   ```

   `--require` lists the layouts that have to be installed. `--sigils` are the characters that may
   stand in front of a token (`#` and `&` in 1C). `--latin-segments` are Latin words that may stand
   inside a word of another layout (`Null` in `ЕстьNull`).

3. Optionally add names from real code: `--bsl <folder>` takes the tokens of the files in a folder
   (this reader understands the syntax of 1C: `//` comments, `"` strings, `|` continuation lines;
   other languages need their own reader in `crates/dictgen/src/programming.rs`, function `tokens`).
   A token goes into the published pack only when it is used in at least `--min-groups` independent
   projects (default 3) and at least `--min-count` times (default 20); a project that shares half of
   its files with another one is the same group. The identifiers of one company never get in.

4. Check it: `dictgen eval-programming --packs ProgrammingLanguages --code rust --seed <seed>
   --bsl <folder> --check` prints how many tokens typed in the wrong layout are repaired and how many
   ordinary tokens would be changed (it has to be below 0.01%); `--prose "ru=<word list>,en=<word list>"`
   also checks ordinary words.

5. Select the language in Settings (Programming language).

The layouts in `--require` have to be installed, otherwise the pack is not active. `--roles` names the
role of each layout (`native` for the layout of the language of the keywords, `latin` for the Latin
one); `ProgrammingLanguages\1c\pack.ini` is a complete example. The detector so far is tested with 1C;
a language whose tokens are Latin only (Rust, C++) is a new case, so measure it with `eval-programming`.

## The local lexicon (your own identifiers)

The names that your own code uses are not in a published pack. Build them on your machine:

```
dictgen programming --local --bsl D:\MyCode --out ProgrammingLanguages\1c\local --min-count 3
```

The file `local\words.fst` is read together with the pack (the words of the pack first, then yours). It is
read into memory, so it can be rebuilt while the daemon runs; the daemon reads it when the packs are
opened again (restart, or save the settings). The folders `ProgrammingLanguages\*\local\` are in
`.gitignore` and are never committed or published.

## The names of the 1C platform

`dictgen mcp-dump --config .mcp.json --out platform-names.json` asks an MCP server (for example
`bsl-platform-context`) for the names of the types, methods and properties of the platform and for the
members of every type. Only the names are saved. The result is cached in the file; run the command again
to continue after an interruption. Give the file to the build with `dictgen programming ... --platform
platform-names.json`.

## Before you publish a pack

- `license` and `sources` in `pack.ini` are filled in and true; the licence of the data allows it
  (see `docs/data-sources.md`, `NOTICE` and `LICENSE-DATA`).
- Every data file is below 5 MB (`dictgen` refuses to write a bigger one).
- There are no identifiers from private code: the public slice is built only with `--min-groups` 3 or more.
