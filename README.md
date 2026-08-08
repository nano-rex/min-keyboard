# Min Keyboard

Minimal offline keyboard project with an Android IME and an independent Linux desktop program. Both editions support English, Simplified Chinese, Traditional Chinese, Japanese, and Korean language modes.

## Current scope
- qwerty on-screen keyboard
- Simplified / Traditional Chinese / English / Japanese / Korean mode cycle
- offline pinyin candidate strip
- CC-CEDICT- and Fcitx-derived offline lexicon asset
- punctuation/symbol toggle
- backspace, space, enter, shift
- AOSP-inspired rounded key surfaces and dark mode toggle
- launcher activity for Android enable/select flow

## Not included
- cloud sync
- online prediction
- handwriting
- voice input
- gesture typing
- full phrase model

## Android build

```bash
./android/gradlew -p android :app:assembleDebug
```

Android files are contained in `android/`.

## Linux version

The Linux version is a standalone desktop application. It does not use IBus,
Fcitx, PyGObject, or a system input-method framework. It provides its own text
editor, on-screen keyboard, candidate recommendations, language toggles, and
configurable hotkeys.

Run it from the repository root:

```bash
python3 linux/min_keyboard.py
```

It requires Python 3 and Tkinter. On Debian or Ubuntu:

```bash
sudo apt install python3 python3-tk
```

See [`linux/README.md`](linux/README.md) for complete usage instructions,
language settings, hotkeys, configuration, and limitations.

## Install and enable
1. Install the APK.
2. Open `Min Keyboard`.
3. Tap `Open input settings` and enable the IME.
4. Tap `Open input picker` and switch to Min Keyboard.

## Dictionary data
- Candidates are loaded from `android/app/src/main/assets/pinyin_lexicon.tsv`.
- English suggestions are loaded from `android/app/src/main/assets/english_words.txt`.
- The pinyin asset can be regenerated with `tools/build_lexicon.py`, combining the existing CC-CEDICT-derived asset with an optional Fcitx YAML export and OpenCC mappings.
- The English asset can be regenerated with `tools/build_english_words.py`, combining the existing list with optional additional word lists.
- Simplified and Traditional candidates are stored side by side for offline lookup.
- Source and license notes are documented in `docs/dictionary-sources.md`.
