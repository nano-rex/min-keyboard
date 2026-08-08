# Min Keyboard

Minimal offline Android keyboard with an AOSP-inspired interface and English, Simplified Chinese, and Traditional Chinese input.

## Current scope
- qwerty on-screen keyboard
- Simplified / Traditional Chinese / English mode cycle
- offline pinyin candidate strip
- CC-CEDICT- and Fcitx-derived offline lexicon asset
- punctuation/symbol toggle
- backspace, space, enter, shift
- AOSP-inspired rounded key surfaces and dark mode toggle
- launcher activity for enable/select flow

## Not included
- cloud sync
- online prediction
- handwriting
- voice input
- gesture typing
- full phrase model

## Build
```bash
./gradlew :app:assembleDebug
```

## Install and enable
1. Install the APK.
2. Open `Min Keyboard`.
3. Tap `Open input settings` and enable the IME.
4. Tap `Open input picker` and switch to Min Keyboard.

## Dictionary data
- Candidates are loaded from `app/src/main/assets/pinyin_lexicon.tsv`.
- English suggestions are loaded from `app/src/main/assets/english_words.txt`.
- The pinyin asset can be regenerated with `tools/build_lexicon.py`, combining the existing CC-CEDICT-derived asset with an optional Fcitx YAML export and OpenCC mappings.
- The English asset can be regenerated with `tools/build_english_words.py`, combining the existing list with an optional IBus libpinyin word list.
- Simplified and Traditional candidates are stored side by side for offline lookup.
- Source and license notes are documented in `docs/dictionary-sources.md`.
