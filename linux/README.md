# Min Keyboard for Linux

Min Keyboard for Linux is an offline IBus input method. It shares the
generated dictionaries and shortcut tables with the Android version. No cloud
service or network connection is required while typing.

## Features

- English word suggestions and expansions, such as `wdym` → `what do you mean`.
- Simplified Chinese pinyin input and abbreviations such as `bzd` → `不知道`.
- Traditional Chinese pinyin candidates.
- Japanese romaji shortcuts, such as `arigatou` → `ありがとう`.
- Korean romanization shortcuts, such as `annyeong` → `안녕`.
- Configurable language enable/disable settings.
- Configurable mode hotkeys.

## Install dependencies

On Debian or Ubuntu:

```bash
sudo apt update
sudo apt install ibus python3 python3-gi gir1.2-ibus-1.0
```

On other distributions, install the equivalent Python 3, IBus, and PyGObject
IBus packages.

## Run from the repository

From the repository root:

```bash
python3 linux/min_keyboard_ibus.py
```

Keep this process running, then restart IBus and open IBus Preferences:

```bash
ibus restart
ibus-setup
```

Add **Min Keyboard** under the input methods list and select it from the
desktop input-method menu.

## Use the keyboard

The default mode is English. Type an abbreviation or word and choose a
candidate from the IBus candidate panel. Press Space to commit the first
candidate, or click a specific candidate.

Default direct language hotkeys are:

| Hotkey | Language |
| --- | --- |
| `Control+1` | English |
| `Control+2` | Simplified Chinese |
| `Control+3` | Traditional Chinese |
| `Control+4` | Japanese |
| `Control+5` | Korean |
| `Control+F12` | Cycle enabled languages |

Examples:

- In English, type `wdym` to get “what do you mean”.
- Switch to Simplified Chinese with `Control+2`, then type `bzd` to get
  `不知道`.
- Switch to Japanese with `Control+4`, then type `arigatou` to get `ありがとう`.
- Switch to Korean with `Control+5`, then type `annyeong` to get `안녕`.

## Configure languages and hotkeys

Create the per-user configuration file:

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

Set a language to `false` to remove it from the cycle and disable its direct
hotkey. Supported modifiers are `Control`, `Alt`, and `Shift`; keys can be
letters, numbers, or names such as `F12`. Restart the IBus engine after
changing the file.

For a temporary mode-cycle hotkey without changing the configuration file:

```bash
python3 linux/min_keyboard_ibus.py --hotkey Alt+F12
```

## System installation

The IBus component XML expects the following system paths:

```bash
sudo install -Dm755 linux/min_keyboard_ibus.py \
  /usr/lib/min-keyboard/min_keyboard_ibus.py
sudo cp -r android/app/src/main/assets /usr/lib/min-keyboard/
sudo install -Dm644 linux/org.nanorex.MinKeyboard.ibus-engine.xml \
  /usr/share/ibus/component/org.nanorex.MinKeyboard.ibus-engine.xml
ibus restart
ibus-setup
```

After installation, add **Min Keyboard** in IBus Preferences. User settings
remain in `~/.config/min-keyboard/config.ini`.

## Troubleshooting

- If Min Keyboard is not listed, confirm the XML exists under
  `/usr/share/ibus/component/`, then run `ibus restart`.
- If candidates are missing, verify that the assets directory exists at
  `/usr/lib/min-keyboard/assets/` for a system installation.
- If hotkeys do not work, check for conflicts with desktop shortcuts and
  restart the IBus engine after editing the configuration file.
