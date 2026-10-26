# Lite Switcher

Lightweight keyboard layout switcher for Windows 10/11 (a Punto Switcher alternative), written in Rust.

Press **Pause/Break** to convert the last typed word to another installed keyboard layout:
type `цщкв` with the Russian layout, press Pause - it becomes `word`; press again - the Ukrainian
variant of the same keys, and so on, back to the original. The layouts are taken from the system
(`GetKeyboardLayoutList`), so any layout installed in Windows works without code changes.

## Hotkeys (configurable)

| Hotkey | Action |
|---|---|
| `Pause` (`Break`) | convert the last typed word to the next layout; press again to continue the cycle |
| `Shift+Pause` | convert the selected text to the next layout (the result stays selected) |
| `Alt+Pause` | invert the case of the selected text |
| *(not assigned)* | switch between `manual` and the last automatic mode |

The tray menu has the "Mode" submenu (Auto literary / Auto programming / Manual; the automatic modes
are being built, until their language packs exist they work as manual) and the items "Settings..."
and "Exit". The mode is stored in `[common] mode`.

In Troubleshooting there are seven check boxes "Do not switch the keyboard layout if any keys have
been pressed" (Backspace, arrows, Delete, Change layout): the automatic switching skips a word if a
checked key was pressed while it was typed. The hotkeys are never blocked by them.

## Languages

The texts are translated into English, Russian and Ukrainian. English is built into the programs;
the other languages are the files of the `Language` folder, which must stay next to the executables
(copy it together with them). Pick the language in the settings (Common / General / Language). To add
a language, copy `Language\English.ini`, translate the values, set `[language] code` and `name` and
put the file into the folder - it shows up in the list. A test checks that all the files have the
same keys, so every new text has to be added to all of them.

## Automatic switching

In the `Auto literary` mode the program watches the word you type and, when you end it with a space,
Enter or an unambiguous punctuation key, checks whether it was typed in the wrong layout (for example
`ghbdtn` instead of `привет`). If it is clearly a word of another installed language, the word is
replaced and the layout of the window is switched; the key that ended the word is typed again after
it. The `Pause` hotkey right after that continues the cycle of layouts. Nothing happens if a key from
Troubleshooting was pressed while the word was typed (Backspace, arrows, Delete, layout change), for
abbreviations in capitals, words with digits, ignored applications and words that are fine in the
current layout. The data are the packs in `LiteraryLanguages` (English, Russian, Ukrainian, built
from frequency lists with `dictgen`, see `NOTICE`).

In the `Auto programming` mode the same happens with the tokens of the selected programming language
(1C: keywords, directives, `#Область`, `&НаСервере`, platform names). Only a dictionary is used, case
does not matter, and a token that is also an ordinary word of the layout you are in is left alone
unless it is a known token of the language typed in the other layout. Hybrids such as `ЕстьNull`
keep their layout. The data are in `ProgrammingLanguagesc`.

In the programming mode the dictionary of 1C is not in the repository (it is built from private code
and from the names of the platform). Build it on your machine, see `docs/adding-a-language.md`
(`dictgen programming`, `dictgen mcp-dump`); your own identifiers go into a local lexicon
(`dictgen programming --local`) that is never committed.

## Language packs

The automatic modes use language packs: `LiteraryLanguages\<code>\` (ordinary languages) and
`ProgrammingLanguages\<id>\` (programming languages) next to the executables, each with a `pack.ini`
and the data files it names. The programs read only the manifests at start; the data of a pack is
opened (memory-mapped) only when the pack is active: a literary pack is active when it is checked in
Settings and a keyboard layout of its language is installed, a programming pack when it is selected
(one at a time) and all the layouts it needs are installed. The format is described in
`docs/auto-switching-spec.md`. For tests the folders can be taken from the folder in the environment
variable `LITE_SWITCHER_DATA_DIR`.

More about adding a language (human or programming): `docs/adding-a-language.md`.

## License

The code is under the MIT licence (`LICENSE`). The data files of the literary packs are under
CC BY-SA 4.0 (`LICENSE-DATA`, sources in `NOTICE` and `docs/data-sources.md`).

## Programs

| Binary | Crate | Purpose |
|---|---|---|
| `lite-switcher.exe` | `crates/daemon` | background daemon: tray icon, keyboard hook, word conversion |
| `lite-switcher-settings.exe` | `crates/settings` | settings editor (also started from the tray menu) |
| - | `crates/core` | shared library: config, layouts, word buffer, hotkeys, autostart |
| - | `crates/lang` | language packs: manifests, discovery, activation, data formats, key table |

## Build and run

```
cargo build --release
target\release\lite-switcher.exe            # daemon (put the settings exe next to it)
target\release\lite-switcher-settings.exe   # settings
cargo test --workspace
cargo bench -p lite-switcher-lang           # timing of the detectors, see docs/benchmarks.md
```

A release folder and zip (programs, `Language`, the packs, licences, docs) is made by
`pwsh -File tools\release.ps1` into `dist\`; add `-WithProgramming` to include the programming packs
you have built (the local lexicon is never included).

## Config

`%APPDATA%\LiteSwitcher\config.ini` is created on the first start. Sections: `[common]`, `[hotkeys]`,
`[troubleshooting]`, `[sounds]` (every key is commented in the file). The daemon re-reads the file
about once a second after it changes; only `log_enabled` needs a restart.

## How it works

The daemon records the *physical keys* (scancodes) of the current word via a low-level keyboard hook.
On the hotkey it erases the word with Backspace, switches the layout of the active window and types
the same keys rendered in the target layout as Unicode (the word conversion does not use the clipboard). The text typed is
never logged. In the automatic modes the log (`log_level = debug`) has the decision, the reason and the
time of it, never the words.

## Limitations

- Windows does not let a normal process see keys typed in windows running as administrator
  (UIPI) - run the daemon elevated if you need that.
- Do not run it together with Punto Switcher while both use the same hotkey.
- The selection actions copy and paste through the clipboard (Ctrl+C / Ctrl+V); the clipboard is
  restored, but applications that read it slowly may paste late.
- Punctuation keys that are letters in another layout (`,` -> `б`) are part of the word.
- The automatic modes do nothing in a standard password field (an edit control with the password style).
  Fields that an application draws itself (browsers, many modern toolkits) cannot be recognised, so
  switch to `Manual` before typing a password there.
- If the tray icon is hidden (`show_tray_icon = false`), stop the daemon from Task Manager.

Status: early development.
