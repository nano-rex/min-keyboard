# Min Keyboard for Linux

This edition is an offline IBus engine. It reuses the generated dictionaries
and shortcut tables from `android/app/src/main/assets/`.

It supports:

- English word suggestions and expansions such as `wdym` → `what do you mean`.
- Simplified and Traditional Chinese pinyin candidates.
- Chinese abbreviations such as `bzd` → `不知道` / `不知道`.
- `Ctrl+F12` to cycle English, Simplified Chinese, and Traditional Chinese.

## Dependencies

Install Python 3, `ibus`, and the PyGObject IBus bindings. On Debian/Ubuntu:

```bash
sudo apt install ibus python3-gi gir1.2-ibus-1.0
```

Run it from the repository checkout:

```bash
python3 linux/min_keyboard_ibus.py
```

For a system installation, copy the script and the `android/app/src/main/assets`
directory to `/usr/lib/min-keyboard/`, install the XML file under
`/usr/share/ibus/component/`, then restart IBus. The XML assumes that install
layout.
