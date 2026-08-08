# Min Keyboard for Linux

Min Keyboard for Linux is a standalone desktop typing program. It does not
use IBus, Fcitx, PyGObject, a desktop input-method service, or a network
connection. The window contains its own text editor, on-screen keyboard,
candidate recommendations, language controls, and hotkeys.

The program is implemented with Python’s standard-library `tkinter` GUI and
reuses the generated dictionary assets from
`android/app/src/main/assets/`.

## Features

- English recommendations and expansions, such as `wdym` → “what do you
  mean”.
- Simplified Chinese pinyin and abbreviations such as `bzd` → `不知道`.
- Traditional Chinese candidates.
- Japanese romaji shortcuts, such as `arigatou` → `ありがとう`.
- Korean romanization shortcuts, such as `annyeong` → `안녕`.
- On-screen QWERTY keyboard and text editor.
- Configurable language enable/disable settings.
- Configurable language hotkeys.

## Requirements

The program requires Python 3 and Tkinter. Tkinter is part of the Python
standard library, but some Linux distributions package its GUI module
separately. On Debian or Ubuntu, install it with:

```bash
sudo apt update
sudo apt install python3 python3-tk
```

No IBus, Fcitx, PyGObject, or other input-method dependency is required.

## Run

From the repository root:

```bash
python3 linux/min_keyboard.py
```

Type in the Min Keyboard window. You can use either the physical keyboard or
the on-screen keys. Recommendations are display-only until you explicitly
click one. Space and Enter preserve the text you typed; they never
auto-correct or auto-select a recommendation.

Examples:

- English: type `wdym` → `what do you mean`.
- Simplified Chinese: switch to `简`, type `bzd` → `不知道`.
- Traditional Chinese: switch to `繁`, type pinyin and select a candidate.
- Japanese: switch to `日`, type `arigatou` → `ありがとう`.
- Korean: switch to `한`, type `annyeong` → `안녕`.

## Language settings and hotkeys

Create a user configuration file:

```bash
mkdir -p ~/.config/min-keyboard
cp linux/config.ini.example ~/.config/min-keyboard/config.ini
```

Edit `~/.config/min-keyboard/config.ini`:

```ini
[hotkeys]
mode = Control+F12
english = Control+1
simplified = Control+2
traditional = Control+3
japanese = Control+4
korean = Control+5

[languages]
english = true
simplified = true
traditional = true
japanese = true
korean = true
```

Set a language to `false` to hide its language button, remove it from the
mode-cycle hotkey, and disable its direct hotkey. Supported modifiers are
`Control`, `Alt`, and `Shift`; keys can be letters, numbers, or names such as
`F12`. Restart the program after editing the configuration file.

Default language hotkeys:

| Hotkey | Language |
| --- | --- |
| `Control+1` | English |
| `Control+2` | Simplified Chinese |
| `Control+3` | Traditional Chinese |
| `Control+4` | Japanese |
| `Control+5` | Korean |
| `Control+F12` | Cycle enabled languages |

## Limitations

This standalone edition types into its own text editor. It does not inject
text into other applications or replace the system-wide keyboard. That keeps
it independent of IBus, Fcitx, Wayland protocols, X11 libraries, and desktop
environment-specific APIs.
