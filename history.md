# History

Versions and changes, newest first. Every change to the project is recorded here.

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
