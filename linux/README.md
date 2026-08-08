# Min Keyboard for Linux

This edition is an offline IBus engine. It reuses the generated dictionaries
and shortcut tables from `android/app/src/main/assets/`.

It supports:

- English word suggestions and expansions such as `wdym` → `what do you mean`.
- Simplified and Traditional Chinese pinyin candidates.
- Chinese abbreviations such as `bzd` → `不知道` / `不知道`.
- `Ctrl+F12` to cycle English, Simplified Chinese, and Traditional Chinese by default.
- User-configurable mode hotkeys, with direct English/Simplified/Traditional hotkeys.

## Dependencies

Install Python 3, `ibus`, and the PyGObject IBus bindings. On Debian/Ubuntu:

```bash
sudo apt install ibus python3-gi gir1.2-ibus-1.0
```

Run it from the repository checkout:

```bash
python3 linux/min_keyboard_ibus.py
```

Enable it in IBus:

```bash
ibus restart
ibus-setup
```

In IBus Preferences, add **Min Keyboard** as an input method. Select it from
the desktop input-method menu, then type normally. In English mode, typing
`wdym` displays the expanded phrase as a candidate. Press Space to commit the
first candidate, or click a candidate in the IBus candidate panel. In Chinese
mode, press `Ctrl+2` and type `bzd`; Traditional mode is `Ctrl+3`.

## User hotkeys

Copy the example preferences into the user configuration directory:

```bash
mkdir -p ~/.config/min-keyboard
cp linux/config.ini.example ~/.config/min-keyboard/config.ini
```

Edit `~/.config/min-keyboard/config.ini` and set any of these values:

```ini
[hotkeys]
mode = Control+F12
english = Control+1
simplified = Control+2
traditional = Control+3
```

Supported modifiers are `Control`, `Alt`, and `Shift`; keys can be letters,
numbers, or names such as `F12`. Restart the IBus engine after changing the
file. For a one-off mode-cycle hotkey, use:

```bash
python3 linux/min_keyboard_ibus.py --hotkey Alt+F12
```

For a system installation, copy the script and the `android/app/src/main/assets`
directory to `/usr/lib/min-keyboard/`, install the XML file under
`/usr/share/ibus/component/`, then restart IBus:

```bash
sudo install -Dm755 linux/min_keyboard_ibus.py /usr/lib/min-keyboard/min_keyboard_ibus.py
sudo cp -r android/app/src/main/assets /usr/lib/min-keyboard/
sudo install -Dm644 linux/org.nanorex.MinKeyboard.ibus-engine.xml \
  /usr/share/ibus/component/org.nanorex.MinKeyboard.ibus-engine.xml
ibus restart
```

The XML uses the system-installed path. User preferences remain at
`~/.config/min-keyboard/config.ini`.
