#!/usr/bin/env python3
"""Build the frequency-ranked runtime dictionaries in android/app/src/main/assets/.

Every subcommand reads source files given on the command line (see
docs/dictionary-sources.md for where to download them) and writes one compact
asset. Run from the repository root, for example:

    python3 tools/build_dictionaries.py english --frequency en_full.txt --spelling /usr/share/dict/american-english
    python3 tools/build_dictionaries.py malay --frequency ms_full.txt
    python3 tools/build_dictionaries.py chinese --jieba dict.txt --frequency zh_cn_full.txt \
        --opencc-characters TSCharacters.txt --opencc-phrases TSPhrases.txt
    python3 tools/build_dictionaries.py japanese --skk SKK-JISYO.L --frequency ja_full.txt
    python3 tools/build_dictionaries.py korean --frequency ko_full.txt

Word lists are written most-frequent first; the keyboard uses the line number
as the frequency rank.
"""
import argparse
import math
import re
import unicodedata
from collections import defaultdict
from pathlib import Path

ASSETS = Path(__file__).resolve().parents[1] / 'android' / 'app' / 'src' / 'main' / 'assets'


def read_frequency(path, minimum=1):
    """Reads a `word count` list (FrequencyWords format) into {word: count}."""
    counts = {}
    for line in Path(path).read_text(encoding='utf-8').splitlines():
        parts = line.rsplit(' ', 1)
        if len(parts) != 2 or not parts[1].isdigit():
            continue
        count = int(parts[1])
        if count >= minimum:
            counts[parts[0]] = counts.get(parts[0], 0) + count
    return counts


def write_lines(name, lines):
    path = ASSETS / name
    path.write_text('\n'.join(lines) + '\n', encoding='utf-8')
    print(f'wrote {path.relative_to(ASSETS.parents[3])}: {len(lines)} lines, {path.stat().st_size // 1024} KiB')


# ---------------------------------------------------------------- English ----

LATIN_WORD = re.compile(r"^[a-z]+(?:['-][a-z]+)*$")
ALWAYS_CAPITALIZED = {'i': 'I', "i'm": "I'm", "i'll": "I'll", "i'd": "I'd", "i've": "I've"}


def build_english(args):
    spelling = {}
    for line in Path(args.spelling).read_text(encoding='utf-8', errors='ignore').splitlines():
        word = unicodedata.normalize('NFC', line.strip())
        if not word or not word.isascii():
            continue
        lower = word.lower()
        if lower.endswith("'s"):
            continue  # possessives are produced by typing, not suggested
        # Prefer the lower-case spelling when both "polish" and "Polish" exist.
        if lower not in spelling or word == lower:
            spelling[lower] = word
    counts = read_frequency(args.frequency, minimum=3)
    ranked = []
    seen = set()
    for word, count in sorted(counts.items(), key=lambda item: -item[1]):
        if not LATIN_WORD.match(word) or word in seen:
            continue
        if word in ALWAYS_CAPITALIZED:
            ranked.append(ALWAYS_CAPITALIZED[word])
        elif word in spelling:
            ranked.append(spelling[word])
        elif count >= args.slang_threshold and len(word) > 1:
            ranked.append(word)  # frequent informal words such as "gonna" or "lol"
        else:
            continue
        seen.add(word)
        if len(ranked) >= args.limit:
            break
    # Keep dictionary words that never appeared in subtitles, after the ranked ones.
    for lower, word in sorted(spelling.items()):
        if len(ranked) >= args.limit + args.extra:
            break
        if lower not in seen and LATIN_WORD.match(lower) and 2 <= len(lower) <= 20:
            ranked.append(word)
            seen.add(lower)
    write_lines('english_words.txt', ranked)


# ------------------------------------------------------------------ Malay ----

def build_malay(args):
    counts = read_frequency(args.frequency, minimum=args.minimum)
    ranked = [word for word, _ in sorted(counts.items(), key=lambda item: -item[1])
              if LATIN_WORD.match(word) and (len(word) > 1 or word in ('a', 'i'))]
    write_lines('malay_words.txt', ranked[:args.limit])


# ---------------------------------------------------------------- Chinese ----

def opencc_to_simplified(characters, phrases):
    char_map, phrase_map = {}, {}
    for path, target in ((characters, char_map), (phrases, phrase_map)):
        for line in Path(path).read_text(encoding='utf-8').splitlines():
            if line and not line.startswith('#') and '\t' in line:
                source, values = line.split('\t', 1)
                target[source] = values.split()[0]

    def convert(text):
        if text in phrase_map:
            return phrase_map[text]
        return ''.join(char_map.get(char, char) for char in text)
    return convert


def build_chinese(args):
    """Re-ranks pinyin_lexicon.tsv so the most frequent words come first."""
    scores = defaultdict(float)
    for line in Path(args.jieba).read_text(encoding='utf-8').splitlines():
        parts = line.split()
        if len(parts) >= 2 and parts[1].isdigit():
            scores[parts[0]] = max(scores[parts[0]], float(parts[1]))
    for word, count in read_frequency(args.frequency).items():
        scores[word] = max(scores[word], float(count))
    to_simplified = opencc_to_simplified(args.opencc_characters, args.opencc_phrases)

    def rank(candidates, convert=None):
        def key(item):
            index, word = item
            score = scores.get(convert(word) if convert else word, 0.0)
            # Frequent words first; unseen words keep their previous relative order.
            return (-math.log1p(score), index)
        return [word for _, word in sorted(enumerate(candidates), key=key)]

    path = ASSETS / 'pinyin_lexicon.tsv'
    lines = []
    for line in path.read_text(encoding='utf-8').splitlines():
        parts = line.split('\t')
        if len(parts) < 3:
            continue
        simplified = rank([value for value in parts[1].split('|') if value])
        traditional = rank([value for value in parts[2].split('|') if value], to_simplified)
        # The fourth column is the top candidate's frequency, used to segment whole sentences.
        top = int(scores.get(simplified[0], 0)) if simplified else 0
        lines.append('\t'.join((parts[0], '|'.join(simplified), '|'.join(traditional), str(top))))
    write_lines('pinyin_lexicon.tsv', lines)


# --------------------------------------------------------------- Japanese ----

HIRAGANA = re.compile(r'^[ぁ-ゖー]+$')


def build_japanese(args):
    """Turns SKK okuri-nasi entries into `reading<TAB>candidate|candidate` lines."""
    frequency = read_frequency(args.frequency)
    raw = Path(args.skk).read_bytes().decode('euc_jis_2004', errors='replace')
    entries = {}
    okuri_nasi = False
    for line in raw.splitlines():
        if line.startswith(';; okuri-nasi entries'):
            okuri_nasi = True
            continue
        if not okuri_nasi or not line or line.startswith(';'):
            continue
        reading, _, rest = line.partition(' /')
        if not HIRAGANA.match(reading) or len(reading) > args.max_reading:
            continue
        candidates = []
        for item in rest.split('/'):
            word = item.split(';', 1)[0].strip()
            if not word or word.startswith('(') or '�' in word or word in candidates:
                continue
            candidates.append(word)
        if candidates:
            entries[reading] = candidates

    # Common kana-only words from subtitles are kept so they can be completed by prefix.
    for word, count in frequency.items():
        if HIRAGANA.match(word) and count >= args.kana_minimum and len(word) > 1:
            entries.setdefault(word, [])

    lines = []
    for reading in sorted(entries):
        # SKK lists candidates in a curated order. Subtitle counts are not used to reorder
        # them because they ignore readings (上 is common, but rarely read かみ).
        candidates = entries[reading][:args.per_reading]
        # The score ranks readings against each other when completing a prefix.
        score = max([frequency.get(reading, 0)] + [frequency.get(word, 0) for word in candidates[:1]])
        lines.append(f"{reading}\t{'|'.join(candidates)}\t{score}")
    write_lines('japanese_lexicon.tsv', lines)


# ----------------------------------------------------------------- Korean ----

INITIALS = ['g', 'kk', 'n', 'd', 'tt', 'r', 'm', 'b', 'pp', 's', 'ss', '', 'j', 'jj', 'ch', 'k', 't', 'p', 'h']
MEDIALS = ['a', 'ae', 'ya', 'yae', 'eo', 'e', 'yeo', 'ye', 'o', 'wa', 'wae', 'oe', 'yo', 'u', 'wo', 'we', 'wi', 'yu', 'eu', 'ui', 'i']
# Final consonants as written before a vowel (liaison) and as pronounced at the end of a syllable.
FINAL_LIAISON = ['', 'g', 'kk', 'gs', 'n', 'nj', 'nh', 'd', 'l', 'lg', 'lm', 'lb', 'ls', 'lt', 'lp', 'lh', 'm', 'b', 'bs', 's', 'ss', 'ng', 'j', 'ch', 'k', 't', 'p', 'h']
FINAL_CODA = ['', 'k', 'k', 'k', 'n', 'n', 'n', 't', 'l', 'k', 'm', 'l', 'l', 'l', 'p', 'l', 'm', 'p', 'p', 't', 't', 'ng', 't', 't', 'k', 't', 'p', 't']
NASAL_BEFORE_N_M = {'k': 'ng', 't': 'n', 'p': 'm'}


def hangul_parts(word):
    parts = []
    for char in word:
        code = ord(char) - 0xAC00
        if not 0 <= code < 11172:
            return None
        parts.append((code // 588, (code % 588) // 28, code % 28))
    return parts


def romanize(word):
    """Revised Romanization with liaison and the common nasal assimilations."""
    parts = hangul_parts(word)
    if not parts:
        return None
    out = []
    for index, (initial, medial, final) in enumerate(parts):
        following = parts[index + 1] if index + 1 < len(parts) else None
        lead = INITIALS[initial]
        if index > 0 and initial == 11 and parts[index - 1][2] not in (0, 21):
            lead = ''  # the previous final consonant has moved onto this syllable
        out.append(lead + MEDIALS[medial])
        if final == 0:
            continue
        if following and following[0] == 11 and final != 21:
            out.append(FINAL_LIAISON[final])
            continue
        coda = FINAL_CODA[final]
        if following and INITIALS[following[0]] in ('n', 'm') and coda in NASAL_BEFORE_N_M:
            coda = NASAL_BEFORE_N_M[coda]
        if following and coda == 'n' and following[0] == 5:
            coda = 'l'
        out.append(coda)
    return ''.join(out)


def transliterate(word):
    """Letter-by-letter romanization, as many people type it."""
    parts = hangul_parts(word)
    if not parts:
        return None
    return ''.join(INITIALS[i] + MEDIALS[m] + FINAL_LIAISON[f] for i, m, f in parts)


def build_korean(args):
    counts = read_frequency(args.frequency, minimum=args.minimum)
    keys = defaultdict(list)
    taken = 0
    for word, count in sorted(counts.items(), key=lambda item: -item[1]):
        if not hangul_parts(word):
            continue
        for key in {romanize(word), transliterate(word)}:
            if key:
                keys[key].append((count, word))
        taken += 1
        if taken >= args.limit:
            break
    lines = []
    for key in sorted(keys):
        words = [word for _, word in sorted(keys[key], key=lambda item: -item[0])][:args.per_key]
        lines.append(f"{key}\t{'|'.join(words)}\t{max(count for count, _ in keys[key])}")
    write_lines('korean_lexicon.tsv', lines)


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    commands = parser.add_subparsers(dest='command', required=True)

    english = commands.add_parser('english')
    english.add_argument('--frequency', required=True, help='FrequencyWords en_full.txt')
    english.add_argument('--spelling', required=True, help='SCOWL word list, e.g. /usr/share/dict/american-english')
    english.add_argument('--limit', type=int, default=80000)
    english.add_argument('--extra', type=int, default=20000, help='unranked dictionary words appended after the ranked list')
    english.add_argument('--slang-threshold', type=int, default=20000)
    english.set_defaults(run=build_english)

    malay = commands.add_parser('malay')
    malay.add_argument('--frequency', required=True, help='FrequencyWords ms_full.txt')
    malay.add_argument('--minimum', type=int, default=3)
    malay.add_argument('--limit', type=int, default=60000)
    malay.set_defaults(run=build_malay)

    chinese = commands.add_parser('chinese')
    chinese.add_argument('--jieba', required=True, help='jieba dict.txt')
    chinese.add_argument('--frequency', required=True, help='FrequencyWords zh_cn_full.txt')
    chinese.add_argument('--opencc-characters', required=True, help='OpenCC TSCharacters.txt')
    chinese.add_argument('--opencc-phrases', required=True, help='OpenCC TSPhrases.txt')
    chinese.set_defaults(run=build_chinese)

    japanese = commands.add_parser('japanese')
    japanese.add_argument('--skk', required=True, help='SKK-JISYO.L (EUC-JP)')
    japanese.add_argument('--frequency', required=True, help='FrequencyWords ja_full.txt')
    japanese.add_argument('--max-reading', type=int, default=12)
    japanese.add_argument('--per-reading', type=int, default=16)
    japanese.add_argument('--kana-minimum', type=int, default=20)
    japanese.set_defaults(run=build_japanese)

    korean = commands.add_parser('korean')
    korean.add_argument('--frequency', required=True, help='FrequencyWords ko_full.txt')
    korean.add_argument('--minimum', type=int, default=5)
    korean.add_argument('--limit', type=int, default=80000)
    korean.add_argument('--per-key', type=int, default=8)
    korean.set_defaults(run=build_korean)

    args = parser.parse_args()
    args.run(args)


if __name__ == '__main__':
    main()
