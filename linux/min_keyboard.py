#!/usr/bin/env python3
"""Standalone Min Keyboard desktop app.

This program intentionally does not integrate with IBus or another input
method framework. It is a self-contained typing window with an on-screen
keyboard, candidate recommendations, language toggles, and hotkeys.
"""

from configparser import ConfigParser
from pathlib import Path
import tkinter as tk


ROOT = Path(__file__).resolve().parents[1]
ASSETS = ROOT / "android" / "app" / "src" / "main" / "assets"
CONFIG_PATH = Path.home() / ".config" / "min-keyboard" / "config.ini"
LANGUAGES = ("english", "simplified", "traditional", "japanese", "korean")
LABELS = {"english": "English", "simplified": "简", "traditional": "繁", "japanese": "日", "korean": "한"}


def load_config():
    values = {"mode": "Control+F12", "english": "Control+1", "simplified": "Control+2", "traditional": "Control+3", "japanese": "Control+4", "korean": "Control+5"}
    enabled = {language: True for language in LANGUAGES}
    parser = ConfigParser()
    if CONFIG_PATH.exists():
        parser.read(CONFIG_PATH, encoding="utf-8")
        if parser.has_section("hotkeys"):
            for name in values:
                if parser.has_option("hotkeys", name): values[name] = parser.get("hotkeys", name)
        if parser.has_section("languages"):
            for language in LANGUAGES:
                if parser.has_option("languages", language): enabled[language] = parser.getboolean("languages", language)
    return values, enabled


def load_tsv(name):
    result = {}
    for line in (ASSETS / name).read_text(encoding="utf-8").splitlines():
        parts = line.split("\t")
        if len(parts) >= 2 and parts[0].strip(): result.setdefault(parts[0].strip(), []).extend(parts[1:])
    return result


class Dictionary:
    def __init__(self):
        self.english = load_tsv("english_shortcuts.tsv")
        self.chinese = load_tsv("pinyin_shortcuts.tsv")
        self.japanese = load_tsv("japanese_shortcuts.tsv")
        self.korean = load_tsv("korean_shortcuts.tsv")
        self.words = [line.strip() for line in (ASSETS / "english_words.txt").read_text(encoding="utf-8").splitlines() if line.strip()]
        self.lexicon = {}
        for line in (ASSETS / "pinyin_lexicon.tsv").read_text(encoding="utf-8").splitlines():
            parts = line.split("\t")
            if len(parts) == 3: self.lexicon[parts[0]] = (parts[1].split("|"), parts[2].split("|"))

    def candidates(self, mode, value):
        if not value: return []
        if mode == "english":
            result = list(self.english.get(value, []))
            result.extend(word for word in self.words if word.startswith(value) and word not in result)
            return result[:32] or [value]
        if mode in ("japanese", "korean"):
            source = self.japanese if mode == "japanese" else self.korean
            result = list(source.get(value, []))
            for key, values in source.items():
                if key.startswith(value): result.extend(item for item in values if item not in result)
                if len(result) >= 32: break
            return result[:32] or [value]
        result = []
        shortcut = self.chinese.get(value)
        if shortcut: result.extend(shortcut[1 if mode == "traditional" else 0].split("|"))
        for key in sorted(self.lexicon):
            if key.startswith(value):
                result.extend(item for item in self.lexicon[key][1 if mode == "traditional" else 0] if item and item not in result)
                if len(result) >= 32: break
        return result[:32] or [value]


class MinKeyboard(tk.Tk):
    def __init__(self):
        super().__init__()
        self.title("Min Keyboard")
        self.geometry("760x560")
        self.minsize(560, 420)
        self.dictionary = Dictionary()
        self.hotkeys, self.enabled = load_config()
        self.mode = self.first_enabled()
        self.composing = ""
        self.candidates = []
        self.text = tk.Text(self, wrap="word", undo=True, font=("sans", 14), height=8)
        self.text.pack(fill="both", expand=True, padx=8, pady=(8, 4))
        self.preedit = tk.Label(self, anchor="w", text="", font=("sans", 12), padx=8)
        self.preedit.pack(fill="x", padx=8)
        self.mode_bar = tk.Frame(self)
        self.mode_bar.pack(fill="x", padx=8, pady=3)
        self.mode_buttons = tk.Frame(self.mode_bar)
        self.mode_buttons.pack(side="left")
        self.candidate_bar = tk.Frame(self.mode_bar)
        self.candidate_bar.pack(side="left", fill="x", expand=True)
        self.status = tk.Label(self.mode_bar, anchor="e")
        self.status.pack(side="right")
        self.keyboard = tk.Frame(self)
        self.keyboard.pack(fill="x", padx=8, pady=(2, 8))
        self.build_mode_buttons()
        self.bind_hotkeys()
        self.bind("<KeyPress>", self.on_key)
        self.build_keyboard()
        self.refresh()

    def first_enabled(self):
        return next((language for language in LANGUAGES if self.enabled.get(language, True)), "english")

    def build_mode_buttons(self):
        for child in self.mode_buttons.winfo_children(): child.destroy()
        for language in LANGUAGES:
            button = tk.Button(self.mode_buttons, text=LABELS[language], command=lambda item=language: self.set_mode(item), width=7)
            button.pack(side="left", padx=2)

    def bind_hotkeys(self):
        for sequence in tuple(self.bindings()): self.unbind(sequence)
        for language in LANGUAGES:
            sequence = self.tk_sequence(self.hotkeys.get(language, ""))
            if sequence and self.enabled.get(language, True): self.bind(sequence, lambda event, item=language: self.set_mode(item) or "break")
        sequence = self.tk_sequence(self.hotkeys.get("mode", ""))
        if sequence: self.bind(sequence, lambda event: self.next_mode() or "break")

    def bindings(self):
        return [self.tk_sequence(value) for value in self.hotkeys.values() if self.tk_sequence(value)]

    @staticmethod
    def tk_sequence(spec):
        parts = [part.strip() for part in spec.split("+") if part.strip()]
        if not parts: return ""
        key = parts[-1]
        modifiers = "-".join(part.title() for part in parts[:-1])
        return "<" + (modifiers + "-" if modifiers else "") + ("Key-" + key if len(key) == 1 else key) + ">"

    def set_mode(self, language):
        if not self.enabled.get(language, True): return
        self.mode = language
        self.composing = ""
        self.refresh()
        self.focus_force()

    def next_mode(self):
        enabled = [language for language in LANGUAGES if self.enabled.get(language, True)] or ["english"]
        self.set_mode(enabled[(enabled.index(self.mode) + 1) % len(enabled)] if self.mode in enabled else enabled[0])

    def build_keyboard(self):
        for child in self.keyboard.winfo_children(): child.destroy()
        rows = ("qwertyuiop", "asdfghjkl", "zxcvbnm")
        for row in rows:
            frame = tk.Frame(self.keyboard); frame.pack(fill="x")
            for key in row: tk.Button(frame, text=key, command=lambda item=key: self.add_text(item), height=2).pack(side="left", fill="x", expand=True, padx=1, pady=1)
        bottom = tk.Frame(self.keyboard); bottom.pack(fill="x")
        for label, action in (("⌫", self.backspace), ("Space", self.space), ("Enter", self.enter)):
            tk.Button(bottom, text=label, command=action, height=2).pack(side="left", fill="x", expand=True, padx=1, pady=1)

    def add_text(self, value):
        self.composing += value.lower()
        self.refresh()

    def on_key(self, event):
        if event.keysym in ("BackSpace", "Delete"): self.backspace(); return "break"
        if event.keysym in ("Return", "KP_Enter"): self.enter(); return "break"
        if event.keysym == "space": self.space(); return "break"
        if event.char and event.char.isalpha(): self.add_text(event.char); return "break"

    def backspace(self):
        if self.composing: self.composing = self.composing[:-1]; self.refresh()
        else: self.text.delete("insert-1c", "insert")

    def commit(self, value):
        self.text.insert("insert", value)
        self.composing = ""
        self.refresh()

    def space(self):
        if self.composing:
            self.text.insert("insert", self.composing)
            self.composing = ""
            self.refresh()
        self.text.insert("insert", " ")

    def enter(self):
        if self.composing:
            self.text.insert("insert", self.composing)
            self.composing = ""
            self.refresh()
        self.text.insert("insert", "\n")

    def refresh(self):
        self.preedit.configure(text=self.composing)
        self.status.configure(text=f"{LABELS[self.mode]}  {self.mode}")
        for child in self.candidate_bar.winfo_children(): child.destroy()
        self.candidates = self.dictionary.candidates(self.mode, self.composing)
        for candidate in self.candidates:
            tk.Button(self.candidate_bar, text=candidate, command=lambda value=candidate: self.commit(value)).pack(side="left", padx=2)


if __name__ == "__main__":
    MinKeyboard().mainloop()
