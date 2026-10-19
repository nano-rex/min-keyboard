# Min Keyboard

[![CI](https://github.com/nano-rex/min-keyboard/actions/workflows/ci.yml/badge.svg)](https://github.com/nano-rex/min-keyboard/actions/workflows/ci.yml)

A minimal offline keyboard: an Android IME and a standalone Linux desktop program. It types English,
Malay, Simplified Chinese, Traditional Chinese, Japanese and Korean, with an optional on-device
AI grammar check for English, Mandarin and Malay. Nothing you type leaves the device: the Android
app has no internet permission.

## Features

| Language | Input | Dictionary |
| --- | --- | --- |
| English | Completions ranked by frequency, typo correction (`teh` → the, `recieve` → receive), contractions (`dont` → don't), shortcuts (`wdym`) | ~74,000 words |
| Malay | Completions and typo correction (`bagaimna` → bagaimana) | ~50,000 words |
| Chinese (简/繁) | Pinyin with the most common characters first (`wo` → 我, `de` → 的), whole sentences (`woaini` → 我爱你, `jintiantianqihenhao` → 今天天气很好), abbreviations (`bzd` → 不知道) | 177,000 pinyin keys |
| Japanese | Romaji → hiragana/katakana with kanji conversion (`nihon` → 日本, `watashi` → 私) | 131,000 readings (SKK) |
| Korean | Romanization → Hangul words (`gamsahamnida` → 감사합니다) with syllable-by-syllable fallback | 97,000 keys |

- **AI grammar check (optional).** Tap ✓ to proofread the current sentence with a small model that
  runs on the phone. A built-in device check recommends a model that fits the hardware, or says
  that none does. The model uses at most a quarter of the RAM and half the CPU cores, and unloads
  when idle, so other apps keep running smoothly. See [docs/ai-grammar-check.md](docs/ai-grammar-check.md).
- **Look and feel.** Rounded keys with depth, light, dark or system theme, key previews, haptic
  feedback, long-press on the top row for digits, highlighted best suggestion, and the language
  name on the space bar.
- Symbol pages, an expandable suggestion panel, auto-space, and per-language enable switches.

## Android

Build a debug APK (needs JDK 17 and the Android SDK; the NDK and CMake are installed by Gradle when missing):

```bash
./android/gradlew -p android :app:assembleDebug
./android/gradlew -p android :app:testDebugUnitTest   # engine, advisor and diff tests
```

Install and enable:

1. Install the APK and open **Min Keyboard**.
2. Tap **Open input settings** and enable the keyboard, then **Choose keyboard**.
3. Optional: under **AI grammar check**, follow the device check to install a model.

## Linux

A standalone Tkinter app with its own editor and on-screen keyboard. It uses the same dictionaries
and does not depend on IBus or Fcitx.

```bash
sudo apt install python3 python3-tk
python3 linux/min_keyboard.py
```

See [linux/README.md](linux/README.md) for hotkeys, configuration and the optional grammar check.

## Repository layout

```text
android/app/src/main/java/   keyboard service, engines (Latin, Pinyin, Japanese, Korean), AI advisor and settings
android/app/src/main/cpp/    llama.cpp-based grammar engine and JNI bridge
android/app/src/main/assets/ dictionaries, shared with the Linux edition
android/app/src/test/        JVM unit tests
linux/                       Linux edition and its tests
tools/                       dictionary builders and a desktop grammar-check CLI
docs/                        dictionary sources and licenses, AI design notes
```

## Dictionaries

The dictionaries are generated from open word lists. Sources, licenses and rebuild commands are in
[docs/dictionary-sources.md](docs/dictionary-sources.md).

## Not included

Cloud sync, online prediction, handwriting, voice input and gesture typing.

## License

GPL-3.0. See [LICENSE](LICENSE). Dictionary data keeps its own licenses, listed in the dictionary sources document.
