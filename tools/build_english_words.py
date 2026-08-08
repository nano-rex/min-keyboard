#!/usr/bin/env python3
import argparse
from pathlib import Path

OUT = Path('app/src/main/assets/english_words.txt')
MAX_WORDS = 50000


def valid(word: str) -> bool:
    if len(word) < 2 or len(word) > 24:
        return False
    return all(ch.isalpha() or ch == "'" for ch in word)


def main() -> None:
    parser = argparse.ArgumentParser(description='Build the offline English suggestion list.')
    parser.add_argument('--source', action='append', type=Path, required=True)
    args = parser.parse_args()
    seen = set()
    words = []
    for source in args.source:
        if not source.exists():
            raise SystemExit(f'missing source file: {source}')
        entries = []
        for line in source.read_text(encoding='utf-8').splitlines():
            word = line.split()[0].strip().lower() if line.split() else ''
            if valid(word) and word not in seen:
                entries.append(word)
        for word in entries:
            seen.add(word)
            words.append(word)
            if len(words) >= MAX_WORDS:
                break
        if len(words) >= MAX_WORDS:
            break
    OUT.parent.mkdir(parents=True, exist_ok=True)
    OUT.write_text('\n'.join(words) + '\n', encoding='utf-8')
    print(f'wrote {OUT} with {len(words)} words')


if __name__ == '__main__':
    main()
