# Min Keyboard for Linux

Min Keyboard for Linux is a standalone desktop typing program. It does not
use IBus, Fcitx, PyGObject, a desktop input-method service, or a network
connection. The window contains its own text editor, on-screen keyboard,
candidate recommendations, language controls, and hotkeys.

The program is implemented with Python’s standard-library `tkinter` GUI and
reuses the generated dictionary assets in `assets/`.

## Features

- English recommendations ranked by frequency, close spellings for typos
  (`teh` → “the”), and expansions such as `wdym` → “what do you mean”.
- Malay recommendations, such as `bagai` → “bagaimana”.
- Simplified and Traditional Chinese pinyin with the most common characters
  first, whole sentences (`woaini` → `我爱你`) and abbreviations such as
  `bzd` → `不知道`.
- Japanese romaji to kana and kanji, such as `nihon` → `日本`.
- Korean romanization to Hangul words, such as `gamsahamnida` → `감사합니다`.
- Optional AI grammar check for English, Mandarin and Malay (see below).
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
malay = Control+6

[languages]
english = true
malay = true
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
| `Control+6` | Malay |
| `Control+F12` | Cycle enabled languages |

## Optional AI grammar check

Install [llama-cpp-python](https://github.com/abetlen/llama-cpp-python) and
download a GGUF model, for example
[Qwen3-1.7B-Q4_K_M.gguf](https://huggingface.co/unsloth/Qwen3-1.7B-GGUF). Then
add to `~/.config/min-keyboard/config.ini`:

```ini
[ai]
model = ~/models/Qwen3-1.7B-Q4_K_M.gguf
```

A **✓ Check grammar** button appears. It proofreads the selection, or the
current line, and shows the suggestion for you to review before replacing. The
model may use at most a quarter of your RAM and half of what is free, runs on
half of the CPU cores, and is unloaded after a minute without checks. See
[`docs/ai-grammar-check.md`](../docs/ai-grammar-check.md).

## Tests

```bash
python3 -m unittest discover -s linux
```

## Limitations

This standalone edition types into its own text editor. It does not inject
text into other applications or replace the system-wide keyboard. That keeps
it independent of IBus, Fcitx, Wayland protocols, X11 libraries, and desktop
environment-specific APIs.
