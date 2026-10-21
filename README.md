# Lite Switcher

Lightweight keyboard layout switcher for Windows 10/11 (a Punto Switcher alternative), written in Rust.

Press **Pause/Break** to convert the last typed word to another installed keyboard layout:
type `цщкв` with the Russian layout, press Pause - it becomes `word`; press again - the Ukrainian
variant of the same keys, and so on, back to the original. The layouts are taken from the system
(`GetKeyboardLayoutList`), so any layout installed in Windows works without code changes.

## Programs

| Binary | Crate | Purpose |
|---|---|---|
| `lite-switcher.exe` | `crates/daemon` | background daemon: tray icon, keyboard hook, word conversion |
| `lite-switcher-settings.exe` | `crates/settings` | settings editor (also started from the tray menu) |
| - | `crates/core` | shared library: config, layouts, word buffer, hotkeys, autostart |

## Build and run

```
cargo build --release
target\release\lite-switcher.exe            # daemon (put the settings exe next to it)
target\release\lite-switcher-settings.exe   # settings
cargo test --workspace
```

## Config

`%APPDATA%\LiteSwitcher\config.ini` is created on the first start. Sections: `[common]`, `[hotkeys]`,
`[troubleshooting]`, `[sounds]` (every key is commented in the file). The daemon re-reads the file
about once a second after it changes; only `log_enabled` needs a restart.

## How it works

The daemon records the *physical keys* (scancodes) of the current word via a low-level keyboard hook.
On the hotkey it erases the word with Backspace, switches the layout of the active window and types
the same keys rendered in the target layout as Unicode (no clipboard involved). The text typed is
never logged.

## Limitations

- Windows does not let a normal process see keys typed in windows running as administrator
  (UIPI) - run the daemon elevated if you need that.
- Do not run it together with Punto Switcher while both use the same hotkey.
- Punctuation keys that are letters in another layout (`,` -> `б`) are part of the word.
- If the tray icon is hidden (`show_tray_icon = false`), stop the daemon from Task Manager.

Status: early development.
