# History

Versions and changes, newest first. Every change to the project is recorded here.

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
