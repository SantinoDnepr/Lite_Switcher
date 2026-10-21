# Lite Switcher

Lightweight keyboard layout switcher for Windows 10/11 (a Punto Switcher alternative), written in Rust.

Press **Pause/Break** to convert the last typed word to the next installed keyboard layout
(e.g. `цщкв` -> `word` -> Ukrainian variant of the same keys).

- `lite-switcher` - background daemon (tray icon, keyboard hook)
- `lite-switcher-settings` - settings editor
- `lite-switcher-core` - shared library (config, layouts, conversion)

Config: `%APPDATA%\LiteSwitcher\config.ini`

Status: early development.
