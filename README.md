# Min Keyboard

[![CI](https://github.com/nano-rex/min-keyboard/actions/workflows/ci.yml/badge.svg)](https://github.com/nano-rex/min-keyboard/actions/workflows/ci.yml)

A minimal offline keyboard: an Android IME and a standalone Linux desktop program. It types English,
Malay, Simplified Chinese, Traditional Chinese, Japanese and Korean. Nothing you type leaves the
device: the Android app has no internet permission.

The Android app is written entirely in Rust with no third-party crates, and it builds without Java,
Gradle or Kotlin. See [How the Android build works](#how-the-android-build-works).

## Features

| Language | Input | Dictionary |
| --- | --- | --- |
| English | Completions ranked by frequency, typo correction (`teh` → the, `recieve` → receive), contractions (`dont` → don't), shortcuts (`wdym`) | ~74,000 words |
| Malay | Completions and typo correction (`bagaimna` → bagaimana) | ~50,000 words |
| Chinese (简/繁) | Pinyin with the most common characters first (`wo` → 我, `de` → 的), whole sentences (`woaini` → 我爱你, `jintiantianqihenhao` → 今天天气很好), abbreviations (`bzd` → 不知道) | 177,000 pinyin keys |
| Japanese | Kana keys in gojūon order, each with its romaji underneath, with ゛ ゜ 小 keys that switch the keys to voiced, p- and small kana (for one kana, or until tapped again after a double tap), a ー key and a hiragana/katakana switch; kanji conversion (にほん → 日本). Romaji on the ABC keys is a setting (`nihon` → 日本) | 131,000 readings (SKK) |
| Korean | Two-set (Dubeolsik) Hangul keys, each with its romanization underneath, composed into syllables, with word completions. Romanization on the ABC keys is a setting (`gamsahamnida` → 감사합니다) | 97,000 keys |

- **Look and feel.** Rounded keys with depth, light, dark or system theme, key previews, haptic
  feedback, long-press on the top row for digits, highlighted best suggestion, and the language
  name on the space bar.
- Shift capitalises the next letter only; tap it twice to lock capitals (⇪).
- **Pronunciation of selected text.** Select Chinese, Japanese or Korean text and the bar above the keys shows its pinyin (with tone marks), romaji or romanization; tap it to see it all, or to type it in place of the selection.
- Symbol pages, an expandable suggestion panel, auto-space, and per-language enable switches.
- Space types the full-width 　 in Chinese and Japanese; a setting switches it to a normal space.
- **Optional AI grammar check** for English, Mandarin and Malay, off by default. Select some
  text and tap **✓ Check grammar** above the keys; the keyboard shows the changes and replaces
  the selection only if you tap **Replace**. It never runs while you type. The model runs on the
  device, in Rust. See [docs/ai-grammar-check.md](docs/ai-grammar-check.md).

## Android

You need Rust (via rustup) and the Android SDK's `build-tools` and `platforms;android-34`.
From the NDK, only the sysroot libraries are used; its compilers are not.

```bash
rustup target add aarch64-linux-android armv7-linux-androideabi x86_64-linux-android
export ANDROID_HOME=~/Android/Sdk          # and ANDROID_NDK_HOME if the NDK is not in $ANDROID_HOME/ndk
cargo run -p apkbuild                      # writes target/apk/min-keyboard.apk
cargo test --workspace                     # engines, keyboard logic, grammar model, dictionary tool, APK tooling
```

`cargo run -p apkbuild -- --help` lists the options, such as `--abi arm64-v8a` for a faster
single-ABI build. Signing uses the system `openssl` command. By default the APK is signed with
`~/.android/debug.keystore`, the same key Android Studio uses, so it installs over earlier debug
builds; the keystore is created if it is missing. For a release, pass `--keystore` and
`--keystore-pass`, or `--key key.pem --cert cert.pem`.

Install and enable:

1. Install the APK (`adb install target/apk/min-keyboard.apk`) and open **Min Keyboard**.
2. Tap **Open input settings** and enable the keyboard, then **Choose keyboard**.

### How the Android build works

Android only binds an input method to a Java class that extends `InputMethodService`, so an APK
still needs a `classes.dex`. `tools/apkbuild` writes that file itself. It declares four small
classes whose methods are `native` and implemented in Rust:

| Class | Purpose |
| --- | --- |
| `ConvoyPinyinImeService` | The input method service |
| `KeyboardView` | One view that Rust lays out, draws through `android.graphics.Canvas`, and handles touches for |
| `SetupActivity` | The settings screen; the layout is `android/res/layout/activity_setup.xml` |
| `NativeCallback` | Click, switch and timer listeners that call back into Rust |

The rest of the build:

1. `cargo` compiles `libminkeyboard.so` for each ABI, and `rust-lld` links it against the NDK sysroot.
2. `aapt2` compiles the manifest and resources and adds the dictionaries. It is a native SDK tool.
3. `apkbuild` adds the DEX and the libraries, aligns the archive (libraries to 16 KiB pages), and
   signs it with APK Signature Scheme v2.

Settings are stored in the same `SharedPreferences` file as the earlier Java app, so they carry
over on upgrade, and an AI model installed by version 0.2.0 is used as it is.

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
engines/          input engines (Latin, Pinyin, Japanese, Korean) and their tests
grammar/          the AI grammar check: GGUF reader, tokenizer and Qwen model on the CPU
android/          the Android app: Rust sources (src/), manifest and resources (res/)
tools/apkbuild/   builds and signs the APK, including the generated classes.dex
tools/dicttool/   builds the dictionaries in assets/
assets/           dictionaries, shared with the Linux edition
linux/            Linux edition and its tests
docs/             dictionary sources and licenses, AI grammar check notes
```

## Dictionaries

The dictionaries are generated from open word lists by `cargo run -p dicttool`. Sources, licenses
and rebuild commands are in [docs/dictionary-sources.md](docs/dictionary-sources.md).

## Not included

Cloud sync, online prediction, handwriting, voice input and gesture typing.

## License

GPL-3.0. See [LICENSE](LICENSE). Dictionary data keeps its own licenses, listed in the dictionary sources document.
