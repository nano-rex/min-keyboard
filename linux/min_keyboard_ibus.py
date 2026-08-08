#!/usr/bin/env python3
"""Small offline IBus engine for Min Keyboard.

It intentionally uses the same generated assets as the Android build so the
two editions share shortcuts and candidate vocabulary.
"""

from pathlib import Path
import sys

import gi

gi.require_version("IBus", "1.0")
from gi.repository import GLib, IBus


ROOT = Path(__file__).resolve().parents[1]
ASSETS = ROOT / "android" / "app" / "src" / "main" / "assets"


def load_tsv(name):
    values = {}
    for line in (ASSETS / name).read_text(encoding="utf-8").splitlines():
        parts = line.split("\t")
        if len(parts) >= 2 and parts[0].strip():
            values[parts[0].strip()] = parts[1:]
    return values


class Dictionary:
    def __init__(self):
        self.english = load_tsv("english_shortcuts.tsv")
        self.pinyin = load_tsv("pinyin_shortcuts.tsv")
        self.words = [line.strip() for line in (ASSETS / "english_words.txt").read_text(encoding="utf-8").splitlines() if line.strip()]
        self.lexicon = {}
        for line in (ASSETS / "pinyin_lexicon.tsv").read_text(encoding="utf-8").splitlines():
            parts = line.split("\t")
            if len(parts) == 3:
                self.lexicon[parts[0]] = (parts[1].split("|"), parts[2].split("|"))

    def english_candidates(self, value):
        results = list(self.english.get(value, []))
        results.extend(word for word in self.words if word.startswith(value) and word not in results)
        return results[:32] or [value]

    def chinese_candidates(self, value, traditional):
        result = []
        shortcut = self.pinyin.get(value)
        if shortcut:
            result.extend(shortcut[1 if traditional else 0].split("|"))
        for key in sorted(self.lexicon):
            if key.startswith(value):
                values = self.lexicon[key][1 if traditional else 0]
                result.extend(item for item in values if item and item not in result)
                if len(result) >= 32:
                    break
        return result[:32] or [value]


class MinKeyboardEngine(IBus.Engine):
    def __init__(self):
        super().__init__()
        self.dictionary = Dictionary()
        self.mode = "english"
        self.composing = ""
        self.candidates = []
        self.lookup = IBus.LookupTable.new(9, 0, True, True)

    def _refresh(self):
        self.candidates = (self.dictionary.english_candidates(self.composing)
                           if self.mode == "english" else
                           self.dictionary.chinese_candidates(self.composing, self.mode == "traditional"))
        self.lookup.clear()
        for candidate in self.candidates:
            self.lookup.append_candidate(IBus.Text.new_from_string(candidate))
        self.update_lookup_table(self.lookup, bool(self.composing), True)
        self.update_preedit_text(IBus.Text.new_from_string(self.composing), len(self.composing), bool(self.composing))

    def _commit(self, text):
        self.commit_text(IBus.Text.new_from_string(text))
        self.composing = ""
        self.candidates = []
        self._refresh()

    def do_candidate_clicked(self, index, button, state):
        if 0 <= index < len(self.candidates):
            self._commit(self.candidates[index])

    def do_process_key_event(self, keyval, keycode, state):
        if state & IBus.ModifierType.CONTROL_MASK and keyval == IBus.KEY_F12:
            self.mode = {"english": "simplified", "simplified": "traditional", "traditional": "english"}[self.mode]
            self.composing = ""
            self._refresh()
            return True
        if state & (IBus.ModifierType.CONTROL_MASK | IBus.ModifierType.MOD1_MASK):
            return False
        if keyval in (IBus.KEY_Return, IBus.KEY_KP_Enter):
            if self.composing:
                self._commit(self.candidates[0] if self.candidates else self.composing)
            return False
        if keyval in (IBus.KEY_space, IBus.KEY_KP_Space):
            if self.composing:
                self._commit(self.candidates[0] if self.candidates else self.composing)
            self.commit_text(IBus.Text.new_from_string(" "))
            return True
        if keyval in (IBus.KEY_BackSpace, IBus.KEY_Delete):
            if self.composing:
                self.composing = self.composing[:-1]
                self._refresh()
                return True
            return False
        character = chr(keyval) if 32 <= keyval < 127 else ""
        if character.isalpha() or (self.mode == "english" and character in "'"):
            self.composing += character.lower()
            self._refresh()
            return True
        return False


def main():
    IBus.init()
    bus = IBus.Bus()
    factory = IBus.Factory.new(bus.get_connection())
    factory.add_engine("min-keyboard", MinKeyboardEngine)
    component = IBus.Component.new("org.nanorex.MinKeyboard", "Min Keyboard", "0.1", "GPL-3.0-or-later", "nano-rex")
    component.add_engine(IBus.EngineDesc.new("min-keyboard", "Min Keyboard", "Offline English and Chinese input", "en", "GPL-3.0-or-later", "nano-rex", "", ""))
    bus.register_component(component)
    GLib.MainLoop().run()


if __name__ == "__main__":
    try:
        main()
    except KeyboardInterrupt:
        sys.exit(0)
