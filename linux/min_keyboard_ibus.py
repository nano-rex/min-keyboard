#!/usr/bin/env python3
"""Small offline IBus engine for Min Keyboard.

It intentionally uses the same generated assets as the Android build so the
two editions share shortcuts and candidate vocabulary.
"""

from pathlib import Path
import argparse
import configparser
import sys

import gi

gi.require_version("IBus", "1.0")
from gi.repository import GLib, IBus


ROOT = Path(__file__).resolve().parents[1]
ASSETS = ROOT / "android" / "app" / "src" / "main" / "assets"
if not ASSETS.exists():
    ASSETS = Path(__file__).resolve().parent / "assets"
DEFAULT_CONFIG = Path.home() / ".config" / "min-keyboard" / "config.ini"
ACTIVE_HOTKEYS = {}
ACTIVE_LANGUAGES = {}


def load_preferences(path, mode_override=None):
    values = {
        "mode": "Control+F12",
        "english": "Control+1",
        "simplified": "Control+2",
        "traditional": "Control+3",
    }
    parser = configparser.ConfigParser()
    if path.exists():
        parser.read(path, encoding="utf-8")
        values.update(dict(parser.items("hotkeys")) if parser.has_section("hotkeys") else {})
    if mode_override:
        values["mode"] = mode_override
    languages = {
        "english": True,
        "simplified": True,
        "traditional": True,
        "japanese": True,
        "korean": True,
    }
    if path.exists():
        parser = configparser.ConfigParser()
        parser.read(path, encoding="utf-8")
        if parser.has_section("languages"):
            for name in languages:
                if parser.has_option("languages", name):
                    languages[name] = parser.getboolean("languages", name)
    return {name: parse_hotkey(spec) for name, spec in values.items()}, languages


def parse_hotkey(spec):
    modifiers = IBus.ModifierType(0)
    parts = [part.strip() for part in spec.split("+") if part.strip()]
    if not parts:
        return 0, modifiers
    for part in parts[:-1]:
        name = part.lower()
        if name in ("ctrl", "control"):
            modifiers |= IBus.ModifierType.CONTROL_MASK
        elif name in ("alt", "mod1"):
            modifiers |= IBus.ModifierType.MOD1_MASK
        elif name == "shift":
            modifiers |= IBus.ModifierType.SHIFT_MASK
    key = parts[-1]
    keyval = ord(key.lower()) if len(key) == 1 else getattr(IBus, "KEY_" + key.upper(), 0)
    return keyval, modifiers


def hotkey_pressed(keyval, state, hotkey):
    expected_key, expected_modifiers = hotkey
    relevant = IBus.ModifierType.CONTROL_MASK | IBus.ModifierType.MOD1_MASK | IBus.ModifierType.SHIFT_MASK
    return keyval == expected_key and (state & relevant) == expected_modifiers


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
        self.japanese = load_tsv("japanese_shortcuts.tsv")
        self.korean = load_tsv("korean_shortcuts.tsv")
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

    def extra_candidates(self, value, japanese):
        source = self.japanese if japanese else self.korean
        result = list(source.get(value, []))
        for key, values in source.items():
            if key.startswith(value):
                result.extend(item for item in values if item not in result)
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
        if self.mode == "english":
            self.candidates = self.dictionary.english_candidates(self.composing)
        elif self.mode in ("simplified", "traditional"):
            self.candidates = self.dictionary.chinese_candidates(self.composing, self.mode == "traditional")
        else:
            self.candidates = self.dictionary.extra_candidates(self.composing, self.mode == "japanese")
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
        if hotkey_pressed(keyval, state, ACTIVE_HOTKEYS.get("english", (0, 0))) and ACTIVE_LANGUAGES.get("english", True):
            self.mode = "english"
            self.composing = ""
            self._refresh()
            return True
        if hotkey_pressed(keyval, state, ACTIVE_HOTKEYS.get("simplified", (0, 0))) and ACTIVE_LANGUAGES.get("simplified", True):
            self.mode = "simplified"
            self.composing = ""
            self._refresh()
            return True
        if hotkey_pressed(keyval, state, ACTIVE_HOTKEYS.get("traditional", (0, 0))) and ACTIVE_LANGUAGES.get("traditional", True):
            self.mode = "traditional"
            self.composing = ""
            self._refresh()
            return True
        if hotkey_pressed(keyval, state, ACTIVE_HOTKEYS.get("mode", (0, 0))):
            order = [name for name in ("english", "simplified", "traditional", "japanese", "korean") if ACTIVE_LANGUAGES.get(name, True)]
            if not order:
                order = ["english"]
            self.mode = order[(order.index(self.mode) + 1) % len(order)] if self.mode in order else order[0]
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
    parser = argparse.ArgumentParser(description="Min Keyboard offline IBus engine")
    parser.add_argument("--config", type=Path, default=DEFAULT_CONFIG)
    parser.add_argument("--hotkey", help="Override the mode-cycle hotkey, e.g. Control+F12")
    args = parser.parse_args()
    global ACTIVE_HOTKEYS, ACTIVE_LANGUAGES
    ACTIVE_HOTKEYS, ACTIVE_LANGUAGES = load_preferences(args.config, args.hotkey)
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
