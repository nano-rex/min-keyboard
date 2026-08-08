#!/usr/bin/env python3
import argparse
import gzip
import re
from collections import defaultdict
from pathlib import Path

OUT = Path('app/src/main/assets/pinyin_lexicon.tsv')
PATTERN = re.compile(r'^(\S+)\s+(\S+)\s+\[([^\]]+)\]')
MAX_PER_KEY = 96
MAX_FCITX_WORDS = 100000


def normalize(pinyin: str) -> str:
    pinyin = pinyin.lower().replace('u:', 'v').replace('ü', 'v')
    pinyin = re.sub(r'\d', '', pinyin)
    pinyin = re.sub(r'\s+', '', pinyin)
    pinyin = re.sub(r"[^a-z']", '', pinyin)
    return pinyin


def add(bucket, field, value):
    if value and value not in bucket[field + '_seen']:
        bucket[field + '_seen'].add(value)
        bucket[field].append(value)


def load_existing(buckets):
    if not OUT.exists():
        return
    for line in OUT.read_text(encoding='utf-8').splitlines():
        key, sep, values = line.partition('\t')
        if not sep:
            continue
        columns = values.split('\t')
        if len(columns) != 2:
            continue
        bucket = buckets[key]
        for value in columns[0].split('|'):
            add(bucket, 'simp', value)
        for value in columns[1].split('|'):
            add(bucket, 'trad', value)


def load_cedict(path, buckets):
    if not path or not path.exists():
        return
    opener = gzip.open if path.suffix == '.gz' else open
    with opener(path, 'rt', encoding='utf-8') as handle:
        for line in handle:
            match = PATTERN.match(line)
            if not match:
                continue
            trad, simp, pinyin = match.group(1), match.group(2), match.group(3)
            key = normalize(pinyin)
            if key:
                add(buckets[key], 'simp', simp)
                add(buckets[key], 'trad', trad)


def load_fcitx_yaml(path, buckets, traditional_converter=None):
    if not path or not path.exists():
        return
    loaded = 0
    for line in path.read_text(encoding='utf-8').splitlines():
        if not line or line.startswith(('#', '---', '...')):
            continue
        fields = line.split()
        if len(fields) < 2 or not any(char.isalpha() for char in fields[1]):
            continue
        word, pinyin = fields[0], ' '.join(fields[1:])
        key = normalize(pinyin)
        if not key or not any('\u4e00' <= char <= '\u9fff' for char in word):
            continue
        add(buckets[key], 'simp', word)
        add(buckets[key], 'trad', traditional_converter(word) if traditional_converter else word)
        loaded += 1
        if loaded >= MAX_FCITX_WORDS:
            break


def opencc_converter(characters, phrases):
    phrase_map = {}
    for path in (phrases,):
        if path and path.exists():
            for line in path.read_text(encoding='utf-8').splitlines():
                if not line or line.startswith('#') or '\t' not in line:
                    continue
                source, target = line.split('\t', 1)
                phrase_map[source] = target.split()[0]
    char_map = {}
    if characters and characters.exists():
        for line in characters.read_text(encoding='utf-8').splitlines():
            if not line or line.startswith('#') or '\t' not in line:
                continue
            source, target = line.split('\t', 1)
            char_map[source] = target.split()[0]

    def convert(text):
        if text in phrase_map:
            return phrase_map[text]
        return ''.join(char_map.get(char, char) for char in text)
    return convert


def main() -> None:
    parser = argparse.ArgumentParser(description='Build the offline Simplified/Traditional pinyin lexicon.')
    parser.add_argument('--cedict', type=Path)
    parser.add_argument('--fcitx-simplified', type=Path)
    parser.add_argument('--opencc-characters', type=Path)
    parser.add_argument('--opencc-phrases', type=Path)
    args = parser.parse_args()

    buckets = defaultdict(lambda: {'simp': [], 'trad': [], 'simp_seen': set(), 'trad_seen': set()})
    load_existing(buckets)
    load_cedict(args.cedict, buckets)
    converter = opencc_converter(args.opencc_characters, args.opencc_phrases)
    load_fcitx_yaml(args.fcitx_simplified, buckets, converter)

    OUT.parent.mkdir(parents=True, exist_ok=True)
    with OUT.open('w', encoding='utf-8') as handle:
        for key in sorted(buckets.keys()):
            bucket = buckets[key]
            bucket['simp'].sort(key=lambda value: (-len(value), value))
            bucket['trad'].sort(key=lambda value: (-len(value), value))
            simp = '|'.join(bucket['simp'][:MAX_PER_KEY])
            trad = '|'.join(bucket['trad'][:MAX_PER_KEY])
            handle.write(f'{key}\t{simp}\t{trad}\n')

    print(f'wrote {OUT} with {len(buckets)} entries')


if __name__ == '__main__':
    main()
