# Dictionary sources

The generated assets are built from these sources:

- Existing CC-CEDICT-derived `pinyin_lexicon.tsv` data, which keeps Simplified and Traditional candidates together.
- Fcitx5 Pinyin Zhwiki, released under the Unlicense: <https://github.com/felixonmars/fcitx5-pinyin-zhwiki>.
- OpenCC conversion tables, used only to derive Traditional candidates for added Fcitx terms: <https://github.com/BYVoid/OpenCC>.
- IBus libpinyin `data/wordlist`, GPL-3.0-or-later: <https://github.com/libpinyin/ibus-libpinyin/blob/main/data/wordlist>.

The build scripts accept these files as inputs and write only the compact runtime assets into `app/src/main/assets/`.
