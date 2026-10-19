"""Candidate engines for the Linux edition, mirroring the Android ones.

They read the same assets as the Android app (android/app/src/main/assets/).
"""

import bisect
import math
from pathlib import Path

ASSETS = Path(__file__).resolve().parents[1] / "android" / "app" / "src" / "main" / "assets"
MAX_CANDIDATES = 32


def _lines(name):
    path = ASSETS / name
    return path.read_text(encoding="utf-8").splitlines() if path.exists() else []


def _tsv(name):
    result = {}
    for line in _lines(name):
        parts = line.split("\t")
        if len(parts) >= 2 and parts[0].strip():
            result.setdefault(parts[0].strip().lower(), []).extend(parts[1:2])
    return result


class LatinEngine:
    """English or Malay: completions by frequency plus close spellings."""

    def __init__(self, words_asset, shortcuts_asset=None):
        self.words = [line.strip() for line in _lines(words_asset) if line.strip()]
        self.rank = {}
        for index, word in enumerate(self.words):
            self.rank.setdefault(word.lower(), index)
        self.sorted = sorted(self.rank)
        self.shortcuts = _tsv(shortcuts_asset) if shortcuts_asset else {}

    def candidates(self, typed):
        key = typed.lower()
        if not key:
            return []
        result = list(self.shortcuts.get(key, []))
        if key in self.rank:
            result.append(self.words[self.rank[key]])
        start = bisect.bisect_left(self.sorted, key)
        completions = []
        for word in self.sorted[start:]:
            if not word.startswith(key):
                break
            if word != key:
                completions.append(self.rank[word])
        completions.sort()
        corrections = sorted((self.rank[word], word) for word in self._edits(key) if word in self.rank)
        for _, word in corrections[:4]:
            result.append(self.words[self.rank[word]])
        result.extend(self.words[rank] for rank in completions[:MAX_CANDIDATES])
        seen, unique = set(), []
        for word in [typed] + result:
            if word not in seen:
                seen.add(word)
                unique.append(word)
        # Keep what was typed visible but not first when a dictionary word matches better.
        if len(unique) > 1 and key not in self.rank and not self.shortcuts.get(key):
            unique[0], unique[1] = unique[1], unique[0]
        return unique[:MAX_CANDIDATES]

    @staticmethod
    def _edits(word):
        letters = "abcdefghijklmnopqrstuvwxyz"
        splits = [(word[:i], word[i:]) for i in range(len(word) + 1)]
        deletes = [a + b[1:] for a, b in splits if b]
        transposes = [a + b[1] + b[0] + b[2:] for a, b in splits if len(b) > 1]
        replaces = [a + c + b[1:] for a, b in splits if b for c in letters]
        inserts = [a + c + b for a, b in splits for c in letters]
        return set(deletes + transposes + replaces + inserts)


class PinyinEngine:
    """Pinyin to Chinese with frequency ranking and whole-sentence segmentation."""

    LOG_CORPUS = math.log(6e7)

    def __init__(self):
        self.lexicon = {}
        for line in _lines("pinyin_lexicon.tsv"):
            parts = line.split("\t")
            if len(parts) >= 3:
                frequency = int(parts[3]) if len(parts) > 3 and parts[3] else 0
                self.lexicon[parts[0]] = (parts[1].split("|"), parts[2].split("|"), frequency)
        self.keys = sorted(self.lexicon)
        self.shortcuts = {}
        for line in _lines("pinyin_shortcuts.tsv"):
            parts = line.split("\t")
            if len(parts) >= 3:
                self.shortcuts[parts[0]] = (parts[1].split("|"), parts[2].split("|"))

    def candidates(self, typed, traditional=False):
        key = typed.lower().replace("'", "")
        if not key:
            return []
        column = 1 if traditional else 0
        result = []
        if key in self.shortcuts:
            result.extend(self.shortcuts[key][column])
        if key not in self.lexicon:
            sentence = self.sentence(key, column)
            if sentence:
                result.append(sentence)
        if key in self.lexicon:
            result.extend(self.lexicon[key][column])
        start = bisect.bisect_right(self.keys, key)
        for other in self.keys[start:]:
            if not other.startswith(key) or len(result) >= MAX_CANDIDATES * 2:
                break
            result.extend(self.lexicon[other][column])
        seen, unique = set(), []
        for word in result:
            if word and word not in seen:
                seen.add(word)
                unique.append(word)
        return unique[:MAX_CANDIDATES] or [typed]

    def sentence(self, key, column):
        """Most probable split into dictionary words (unigram model); None if impossible."""
        n = len(key)
        best = [math.inf] * (n + 1)
        back = [None] * (n + 1)
        best[0] = 0.0
        for i in range(n):
            if best[i] == math.inf:
                continue
            for j in range(i + 1, min(n, i + 30) + 1):
                entry = self.lexicon.get(key[i:j])
                if entry is None and j == n:
                    entry = self._completion(key[i:j])
                if entry and entry[column]:
                    cost = best[i] + self.LOG_CORPUS - math.log(entry[2] + 1.0)
                    if cost < best[j]:
                        best[j] = cost
                        back[j] = (i, entry[column][0])
        if best[n] == math.inf:
            return None
        words, at = [], n
        while at > 0:
            at, word = back[at]
            words.append(word)
        return "".join(reversed(words)) if len(words) > 1 else None

    def _completion(self, prefix):
        start = bisect.bisect_right(self.keys, prefix)
        best = None
        for other in self.keys[start:start + 200]:
            if not other.startswith(prefix):
                break
            entry = self.lexicon[other]
            if best is None or entry[2] > best[2]:
                best = entry
        return best


ROMAJI = {
    "a": "あ", "i": "い", "u": "う", "e": "え", "o": "お",
    "ka": "か", "ki": "き", "ku": "く", "ke": "け", "ko": "こ", "ga": "が", "gi": "ぎ", "gu": "ぐ", "ge": "げ", "go": "ご",
    "sa": "さ", "si": "し", "shi": "し", "su": "す", "se": "せ", "so": "そ", "za": "ざ", "zi": "じ", "ji": "じ", "zu": "ず", "ze": "ぜ", "zo": "ぞ",
    "ta": "た", "ti": "ち", "chi": "ち", "tu": "つ", "tsu": "つ", "te": "て", "to": "と", "da": "だ", "di": "ぢ", "du": "づ", "de": "で", "do": "ど",
    "na": "な", "ni": "に", "nu": "ぬ", "ne": "ね", "no": "の", "ha": "は", "hi": "ひ", "hu": "ふ", "fu": "ふ", "he": "へ", "ho": "ほ",
    "ba": "ば", "bi": "び", "bu": "ぶ", "be": "べ", "bo": "ぼ", "pa": "ぱ", "pi": "ぴ", "pu": "ぷ", "pe": "ぺ", "po": "ぽ",
    "ma": "ま", "mi": "み", "mu": "む", "me": "め", "mo": "も", "ya": "や", "yu": "ゆ", "yo": "よ",
    "ra": "ら", "ri": "り", "ru": "る", "re": "れ", "ro": "ろ", "la": "ら", "li": "り", "lu": "る", "le": "れ", "lo": "ろ",
    "wa": "わ", "wo": "を", "fa": "ふぁ", "fi": "ふぃ", "fe": "ふぇ", "fo": "ふぉ",
    "ja": "じゃ", "ju": "じゅ", "je": "じぇ", "jo": "じょ", "sha": "しゃ", "shu": "しゅ", "she": "しぇ", "sho": "しょ",
    "cha": "ちゃ", "chu": "ちゅ", "che": "ちぇ", "cho": "ちょ", "-": "ー",
}
for _consonant, _kana in (("k", "き"), ("g", "ぎ"), ("s", "し"), ("z", "じ"), ("t", "ち"), ("d", "ぢ"), ("n", "に"),
                          ("h", "ひ"), ("b", "び"), ("p", "ぴ"), ("m", "み"), ("r", "り")):
    for _vowel, _small in (("ya", "ゃ"), ("yu", "ゅ"), ("yo", "ょ")):
        ROMAJI.setdefault(_consonant + _vowel, _kana + _small)


def to_hiragana(romaji, finish=True):
    """Romaji to hiragana, matching the Android converter (see JapaneseEngine.java)."""
    out, i, text = [], 0, romaji.lower()
    while i < len(text):
        c = text[i]
        nxt = text[i + 1] if i + 1 < len(text) else ""
        if nxt == c and c.isalpha() and c not in "aeioun":
            out.append("っ"); i += 1; continue
        if c == "n" and nxt:
            after = text[i + 2] if i + 2 < len(text) else ""
            if nxt == "n":
                out.append("ん"); i += 1 if after and after in "aeiouy" else 2; continue
            if nxt == "'":
                out.append("ん"); i += 2; continue
            if nxt.isalpha() and nxt not in "aeiouy":
                out.append("ん"); i += 1; continue
        for length in (4, 3, 2, 1):
            piece = ROMAJI.get(text[i:i + length])
            if piece:
                out.append(piece); i += length; break
        else:
            out.append(c); i += 1
    result = "".join(out)
    return result[:-1] + "ん" if finish and result.endswith("n") else result


class JapaneseEngine:
    def __init__(self):
        self.lexicon = {}
        for line in _lines("japanese_lexicon.tsv"):
            parts = line.split("\t")
            if len(parts) >= 2:
                self.lexicon[parts[0]] = [word for word in parts[1].split("|") if word]
        self.shortcuts = _tsv("japanese_shortcuts.tsv")

    def candidates(self, typed):
        if not typed:
            return []
        kana = to_hiragana(typed)
        katakana = "".join(chr(ord(c) + 0x60) if "ぁ" <= c <= "ゖ" else c for c in kana)
        result = list(self.shortcuts.get(typed.lower(), [])) + self.lexicon.get(kana, []) + [kana, katakana]
        seen, unique = set(), []
        for word in result:
            if word not in seen:
                seen.add(word)
                unique.append(word)
        return unique[:MAX_CANDIDATES]


class KoreanEngine:
    def __init__(self):
        self.lexicon = {}
        for line in _lines("korean_lexicon.tsv"):
            parts = line.split("\t")
            if len(parts) >= 2:
                self.lexicon[parts[0]] = (parts[1].split("|"), int(parts[2]) if len(parts) > 2 and parts[2] else 0)
        self.keys = sorted(self.lexicon)
        self.shortcuts = _tsv("korean_shortcuts.tsv")

    def candidates(self, typed):
        key = typed.lower()
        if not key:
            return []
        result = list(self.lexicon.get(key, ([], 0))[0]) + list(self.shortcuts.get(key, []))
        start = bisect.bisect_right(self.keys, key)
        completions = []
        for other in self.keys[start:start + 3000]:
            if not other.startswith(key):
                break
            completions.append(self.lexicon[other])
        completions.sort(key=lambda entry: -entry[1])
        result.extend(words[0] for words, _ in completions[:20])
        seen, unique = set(), []
        for word in result:
            if word not in seen:
                seen.add(word)
                unique.append(word)
        return unique[:MAX_CANDIDATES] or [typed]
