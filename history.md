# History

Versions and changes, newest first. Every change to the project is recorded here.

## 0.9.3 - 2026-10-26

### Changed
- The row of tabs in Settings scrolls when the window is too narrow for all the tabs: `<` and `>` buttons
  appear at its ends (and the mouse wheel or a drag over the row scrolls it).

## 0.9.2 - 2026-10-26

### Changed
- The "Open the config folder" button moved from the Troubleshooting tab to the footer of the settings window,
  to the right of Save and Reload.

## 0.9.1 - 2026-10-26

### Changed
- `mcp-dump` restarts the platform server when it dies on a query and skips that query; the crawl of the
  1C platform is complete: 1204 types, 447 methods, 86 properties, 3741 members (5,298 distinct names).
- The local 1C pack was rebuilt with the platform layer (11,762 words: 256 seed, 5,298 platform, 7,502 mined).
  Measured: seed 1464 of 1464, replay of 1,000,000 tokens of real code 1 text change (0.0001%, same as
  before), 60,000 ordinary words 0 changed, about 22 us per decision. The thresholds are met.
- The release script was run: `dist\Lite_Switcher-0.9.0.zip` has no programming packs (no `-WithProgramming`).

## 0.9.0 - 2026-10-26

Stage 6 of the automatic switching: hardening and delivery.

### Added
- The automatic modes do nothing when the focus is in a standard password field (an edit control with
  the password style); a scenario in `tools/e2e/auto-literary.ps1` checks it. Fields drawn by applications
  themselves cannot be recognised (README, Limitations).
- Benchmarks (`cargo bench -p lite-switcher-lang`, criterion) and `docs/benchmarks.md`: literary decisions
  1-13 us, 1C decisions 12-170 us (6 to 40 keys), the key table 22 ns per 6 keys, pack discovery 0.7 ms,
  opening a pack 2.6 ms, the data of all packs 1.5 MB (NFR-1...NFR-4 met).
- `tools/release.ps1`: release build into `dist\Lite_Switcher-<version>` and a zip with the programs, `Language`,
  the packs, licences and docs; the programming packs only with `-WithProgramming`, never the local lexicon.
- `mcp-dump` skips a query that the server does not answer in 60 seconds (the server of the platform
  hangs on some queries) and stops, with the cache saved, after three such queries in a row.

### Changed
- The 1C detector renders the keys of a token once per layout instead of for every reading: 24 keys 113 -> 77 us,
  40 keys 265 -> 170 us.
- Logging was checked: the log has layout ids, lengths, reasons and times, never typed words (README).

## 0.8.0 - 2026-10-26

Stage 5 of the automatic switching: sources and delivery.

### Added
- `LICENSE` (MIT, code); `NOTICE` describes the data of the 1C pack. `docs/adding-a-language.md`: how to add a
  literary or a programming language (Rust, C++ and others), with the commands of `dictgen`.
- `dictgen mcp-dump`: reads the names of the 1C platform (types, global methods and properties, members of
  every type) from the MCP server of `.mcp.json`; only names are saved; the result is cached in the file and a
  second run asks only for what is missing. `dictgen programming --platform <dump.json>` adds them as a layer
  under the mined names (flag `platform`).
- The local lexicon: `dictgen programming --local --bsl <folder> --out <pack>/local`. A programming pack reads
  `local\words.fst` next to the published words (read into memory, so it can be rebuilt while the daemon runs;
  a broken file is ignored). Never committed (`.gitignore`).
- `dictgen` refuses to write a published data file bigger than 5 MB.
- Tests: the public pack has no identifier used by fewer than `--min-groups` independent projects, the local
  one has it; the local layer of a pack.

### Changed
- The 1C dictionary (`words.fst`, `pack.ini`) is no longer committed: it is built from private code
  (decision of the owner). Only `ProgrammingLanguages/1c/seed.txt` is in git; the rest is in `.gitignore`.
- The data licence CC BY-SA 4.0 for the literary packs is accepted by the owner.

## 0.7.0 - 2026-10-26

Stage 4 of the automatic switching: the programming mode (`Auto programming`, language 1C).

### Added
- The programming detector (`lite-switcher-lang`): dictionary only, case-insensitive. A token is
  looked up as a whole and as a split (a part in one layout, a part in another) in every layout the
  pack needs; a sigil (`#`, `&`) followed by a known word is accepted; a word of up to three
  letters has to be a keyword of the language; only trailing punctuation keys are tried as
  punctuation; hybrids like `ЕстьNull` keep their layout.
- `ProgrammingLanguagesc`: pack with 7,756 words built from a seed list (keywords, directives,
  preprocessor) and from a public slice of real code (a token counts only if it is used in at least
  3 independent project groups and occurs at least 20 times).
- `dictgen programming`, `dictgen eval-programming` (seed check, replay of real code, ordinary words
  with `--prose`), `dictgen list`.
- The daemon replaces a token in the programming mode like in the literary one (same input gate,
  trigger handling and cycle continuation); `target_hkl` of the plan lets the layout be chosen by
  the pack.
- `tools/e2e/auto-programming.ps1`: live scenarios (run only when nobody works at the PC).
- `.gitignore`: `ProgrammingLanguages/*/local/` (the local lexicon, never committed).

### Measured
- Seed: 1464 of 1464 decisions correct (244 forms, 3 case variants, 2 layouts).
- Replay of 1,000,000 tokens of real code: text changed 1 (0.0001%, `правыйшифт` -> `правыйiban`),
  threshold 0.01%; about 27 microseconds per decision.
- 60,000 frequent ordinary words (ru, en) typed correctly: 0 changed.

## 0.6.0 - 2026-10-26

Stage 3 of the automatic switching: the literary mode (`Auto literary`) works.

### Added
- Automatic replacement of a word typed in the wrong layout. The word is checked when it is ended
  with a space, Enter or a punctuation key that is never a letter in the installed layouts; the
  decision takes a few microseconds. The ending key is swallowed and typed again after the
  replacement (a punctuation key as the character of the new layout, Enter as Enter). The convert
  hotkey continues the cycle from the new layout.
- Input gate: the keys that the user types while a word is being replaced are held back and replayed
  afterwards (at most 200 ms).
- The literary detector (`lite-switcher-lang`): rules (letters on punctuation keys, forbidden
  bigrams, abbreviations in capitals, exceptions), a dictionary word beats a word that only looks like
  one, letter trigram costs with margins, punctuation around the word is tried both as letters and as
  punctuation, the language of the previous word breaks ru/uk ties, a word that looks the same in the
  other layout is left alone.
- The Troubleshooting flags take effect: Backspace, arrows, Delete and a layout change while a word is
  typed make it "unclean" and it is skipped (the hotkeys are never blocked).
- `dictgen` tool: `dictgen literary` builds a pack from a word-count list, `dictgen eval` measures
  false switches and restored words on frequent and rare words and checks the acceptance thresholds.
- Packs `LiteraryLanguages\en`, `ru`, `uk` (46-48 thousand words each, 1.3 MB in total) built from
  FrequencyWords 2018; `NOTICE`, `LICENSE-DATA`.
- `tools\e2e\auto-literary.ps1`: a live check of the automatic mode in a text box (run it when
  nobody works at the computer).

### Changed
- `WordBuffer` keeps the keys typed after the word (punctuation too), the layout at the start of the
  word and the "unclean" bits; in an automatic mode the keys are classified by the key table
  (letter, separator, ambiguous) instead of asking Windows for every key.
- Delete no longer clears the typed word (it removes what is after the caret).
- Measured accuracy replaces the prototype figures in the architecture and the specification.

## 0.5.0 - 2026-10-26

Stage 2 of the automatic switching: the infrastructure of the language packs. There are no packs yet,
so the automatic modes still work as manual.

### Added
- New crate `lite-switcher-lang` (no WinAPI): the manifest `pack.ini` with validation (required fields,
  the code must match the folder name, supported data format, file names cannot leave the pack
  folder), discovery of the packs in `LiteraryLanguages` and `ProgrammingLanguages` (only manifests
  are read; broken packs are skipped and reported), activation rules, opening of the data (`words.fst`
  through memory mapping, `ngram3.bin`, `exceptions.txt`) with SHA-256 checks, and the key rendering
  table with key classes (letter / separator / ambiguous) that replaces the slow `ToUnicodeEx`.
- Daemon: packs are found and the data of the active ones is opened in a background thread whenever
  the config changes; the finished set replaces the old one atomically, the data of packs that are no
  longer active is released. If the selected automatic mode has no active pack, the tray tooltip says
  that it works as manual.
- Settings: tabs "Literary language" (check boxes, "Found N, active M", a note for packs without an
  installed layout) and "Programming language" (radio buttons, one language); the hint in Common is
  shown only when the selected mode has no active pack. The check boxes are stored in
  `[literary_language] enabled` (`auto` when all found packs are checked).
- `LITE_SWITCHER_DATA_DIR` environment variable: look for the data folders only there (tests).

## 0.4.0 - 2026-10-26

Stage 1 of the automatic switching (`docs/auto-switching-spec.md`): modes and the Troubleshooting key
flags. The automatic modes themselves are not built yet (they work as manual).

### Added
- `[common] mode = manual | auto_literary | auto_programming` (default `manual`). The old
  `autoswitch` flag is read once when `mode` is absent (`true` -> `auto_literary`) and is not written
  any more.
- Tray menu: the "Auto-switching" check box is replaced by the "Mode" submenu with the items
  "Auto literary", "Auto programming" and "Manual" (the check mark follows the current mode).
- Settings, Common / General: the "Mode" drop-down list instead of the check box.
- Settings, Troubleshooting: the group "Do not switch the keyboard layout if any keys have been
  pressed" with seven check boxes (Backspace, Left/Right/Up/Down arrow, Delete, Change layout) and the
  matching `no_switch_if_*` keys in `[troubleshooting]` (all checked by default). The values are
  stored; they take effect when the automatic switching is built.
- Config sections `[literary_language] enabled` (`auto` or pack codes) and
  `[programming_language] selected` (one pack code) for the next stage.

### Changed
- The toggle hotkey (`toggle_autoswitch`) now switches between `manual` and the last automatic mode.
- The old texts `tray.auto_switching`, `common.auto_switching`, `common.auto_switching_hint` are removed
  from the language files; the new ones are used.

## 0.3.4 - 2026-10-26

### Changed
- `docs/auto-switching-architecture.md` (version 2) and `docs/auto-switching-spec.md` (version 1.1)
  reworked after `prompts/prompt004.md`: one programming language selected with a radio button;
  auto-replacement also on Enter and unambiguous punctuation (the trigger key is swallowed and
  typed again after the replacement); the convert hotkey continues the cycle after an automatic
  replacement; a group of seven "Do not switch the keyboard layout if any keys have been pressed"
  check boxes in Troubleshooting (Backspace, arrows, Delete, Change layout); decisions on the
  corpora, committing frequency slices and the default mode.
- Estimates replaced by measurements: size and speed of the dictionaries, key rendering and the
  hybrid segmentation, statistics of the 1C corpus per project (23 folders = 12-16 independent
  groups), coverage of the hybrid-word rule (about 60 %), and a prototype of the literary cascade
  on word-frequency lists (accuracy targets in the specification are based on it).
- Added a readiness check (architecture, section 12) with the list of changes of existing code and
  the readiness of every stage.

### Added
- `docs/data-sources.md`: sources of data for the language packs, verified licences (what is used,
  what is excluded and why), attribution template.
- Translations of the new texts (Mode menu, language tabs, programming language radio list,
  Troubleshooting group) in `Language\English.ini`, `Russian.ini`, `Ukrainian.ini`. They are not used
  by the program yet (planned for stage 1); no program code changed.

## 0.3.3 - 2026-10-25

### Added
- `docs/auto-switching-architecture.md`: architecture decisions for the automatic switching (modes,
  language packs, literary and programming detectors, data formats, speed, risks), with the
  measurements that justify them (MCP server probes, statistics of the 1C `.bsl` corpus).
- `docs/auto-switching-spec.md`: technical specification (requirements, config keys, data formats,
  acceptance thresholds, stages) for the automatic switching. No program code changed.

## 0.3.2 - 2026-10-25

### Changed
- The tray menu item is written with a capital letter: "Auto-switching" / "Автопереключение" /
  "Автоперемикання" (hand edit of the language files, the test was updated).
- `.gitignore`: `.mcp.json` and `.MCP` are ignored too.

## 0.3.1 - 2026-10-23

### Changed
- The names of the keyboard layouts are taken from Windows again, in the language of the system
  (e.g. `Русский (Россия)`), as before 0.3.0. They are not translated by the program, the `[layouts]`
  groups are removed from the language files.

## 0.3.0 - 2026-10-23

### Added
- Internationalization of every text of the daemon (tray menu, tooltip) and of the settings
  (window title, tabs, labels, buttons, hints, status messages, hotkey names) into English, Russian
  and Ukrainian.
- `Language` folder with one `.ini` file per language (`English.ini`, `Russian.ini`, `Ukrainian.ini`),
  split into groups (`[tray]`, `[tabs]`, `[buttons]`, `[common]`, `[hotkeys]`, `[troubleshooting]`,
  `[sounds]`, `[status]`, `[app]`). The folder is searched next to the executables and in
  the parent folders (so it is found when the programs run from `target\debug`).
- English is built into the binaries (`English.ini` is embedded at compile time) and is the fallback
  for missing texts, so the programs work even without the `Language` folder.
- "Language" drop-down in the "General" group of the "Common" tab. Its items are English plus the
  languages found in the `Language` folder (a file is a language if it has `[language] code` and
  `name`), so a new translation appears after dropping a file into the folder. The texts of the
  settings change immediately when another language is picked.
- `[common] language` in `config.ini` (`en`, `ru`, `uk`...). The daemon reads it at start and
  updates the tray menu when the config changes.
- Tests: every language file has exactly the keys of `English.ini` (and the same `{placeholders}`),
  and every text key used in the code exists - a new text must be added to all three files.

### Changed
- The tray menu item and the check box in the settings are called "auto-switching" (translated:
  "автопереключение" / "автоперемикання").
- Keyboard layouts were shown with English names (`English (USA)`...); reverted in 0.3.1.

## 0.2.0 - 2026-10-23

### Added
- Tray menu item "Автоперемикання" with a check mark. The automatic switching itself is a stub for
  now: the flag is stored in `config.ini` (`[common] autoswitch`, default `false`) and shown on the
  "Common" tab of the settings. The flag and the menu stay in sync with the file in both directions.
- Hotkey `Shift+Break` (`convert_selection`): converts the selected text to the next layout of the
  list. The layout the text was typed in is detected from the text itself (the layout that can type
  most of its letters, the current layout wins a tie); the result stays selected, so pressing the
  hotkey again continues the cycle.
- Hotkey `Alt+Break` (`invert_case`): inverts the case of the selected text.
- Hotkey `toggle_autoswitch`: turns the automatic switching on / off. No key is assigned by default;
  set it in the settings or in `[hotkeys]`.
- The hotkeys of the selection actions can be cleared (empty value in `config.ini`) and are edited on
  the "Hotkeys" tab; the settings refuse to save two actions with the same hotkey.
- The selected text is read with Ctrl+C and replaced with Ctrl+V; the previous clipboard content
  (all byte-copyable formats) is restored afterwards. It runs in a background thread, so the
  keyboard hook never waits for the application.
- Alt / Win based hotkeys no longer open the menu bar / Start menu of the application.

### Changed
- The "Pause" item is removed from the tray menu: layouts are switched either automatically (to be
  done) or manually by the hotkeys. To stop the program use "Exit".
- The icon no longer changes colour (it showed the removed pause state).
- `Pause` and `Break` are the same key; the config accepts both names and writes `Pause`.

### Fixed
- Turning autostart off removed the autostart entry of any copy of the program; now only the entry
  that points to the running executable is removed (and the registry key is opened for writing).

## 0.1.1 - 2026-10-22

### Fixed
- The conversion cycle no longer starts from the beginning of the layout list. After the hotkey the
  word goes to the layout that follows the current one in the list, then to the next ones, and the
  last step returns to the original layout. With the list `en, ru, uk`: typed in `uk` -> `en` -> `ru`
  -> `uk`; typed in `ru` -> `uk` -> `en` -> `ru`.
- The current layout is now read from the window that has the keyboard focus (`GetGUIThreadInfo`),
  not only from the thread of the foreground top-level window. In some applications the latter
  reports a wrong layout, which made the cycle start from the wrong place. The layout change request
  is sent to the focused window too.

### Changed
- The list of layouts and their order are taken when the daemon starts (and when `config.ini`
  changes), not on every hotkey press.
- Debug log (`log_level = debug`) shows the layout list and the cycle order of each conversion.
- `LITE_SWITCHER_TEST_INSTANCE` environment variable lets a test copy of the daemon run next to
  the real one (different single-instance mutex). For development only.

## 0.1.0 - 2026-10-21

First working version.

### Added
- Cargo workspace: `lite-switcher-core` (library), `lite-switcher` (daemon),
  `lite-switcher-settings` (settings editor).
- Hotkey (default `Pause`, configurable) converts the last typed word to another keyboard layout;
  repeated presses cycle through the layouts and back to the original one. Spaces typed after the
  word are kept.
- Layouts are taken from the system (`GetKeyboardLayoutList`); no languages are hard-coded.
- `%APPDATA%\LiteSwitcher\config.ini` with the sections `common`, `hotkeys`, `troubleshooting`,
  `sounds`; created with defaults and comments on the first start; hot reload.
- Daemon: low-level keyboard and mouse hooks, tray icon (Settings, Pause, Exit), single instance,
  ignored applications list, optional log file and sound, autostart with Windows.
- Settings editor (egui) with a tab for each config section, hotkey capture and layout selection.
- Unit tests for the config, hotkeys, layouts and the word buffer; `README.md`.
