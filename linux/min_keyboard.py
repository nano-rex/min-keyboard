#!/usr/bin/env python3
"""Standalone Min Keyboard desktop app.

This program intentionally does not integrate with IBus or another input
method framework. It is a self-contained typing window with an on-screen
keyboard, candidate recommendations, language toggles, and hotkeys.
"""

from configparser import ConfigParser
from pathlib import Path
import sys
import threading
import tkinter as tk

sys.path.insert(0, str(Path(__file__).resolve().parent))
from engines import JapaneseEngine, KoreanEngine, LatinEngine, PinyinEngine  # noqa: E402
from grammar import GrammarChecker, detect_language  # noqa: E402


CONFIG_PATH = Path.home() / ".config" / "min-keyboard" / "config.ini"
LANGUAGES = ("english", "malay", "simplified", "traditional", "japanese", "korean")
LABELS = {"english": "English", "malay": "BM", "simplified": "简", "traditional": "繁", "japanese": "日", "korean": "한"}


def load_config():
    values = {"mode": "Control+F12", "english": "Control+1", "simplified": "Control+2", "traditional": "Control+3", "japanese": "Control+4", "korean": "Control+5", "malay": "Control+6"}
    enabled = {language: True for language in LANGUAGES}
    model = ""
    parser = ConfigParser()
    if CONFIG_PATH.exists():
        parser.read(CONFIG_PATH, encoding="utf-8")
        if parser.has_section("hotkeys"):
            for name in values:
                if parser.has_option("hotkeys", name): values[name] = parser.get("hotkeys", name)
        if parser.has_section("languages"):
            for language in LANGUAGES:
                if parser.has_option("languages", language): enabled[language] = parser.getboolean("languages", language)
        if parser.has_option("ai", "model"):
            model = str(Path(parser.get("ai", "model")).expanduser())
    return values, enabled, model


class Dictionary:
    """Loads each language's engine the first time that language is used."""

    def __init__(self):
        self.engines = {}

    def engine(self, mode):
        if mode not in self.engines:
            factories = {
                "english": lambda: LatinEngine("english_words.txt", "english_shortcuts.tsv"),
                "malay": lambda: LatinEngine("malay_words.txt"),
                "simplified": PinyinEngine, "traditional": lambda: self.engine("simplified"),
                "japanese": JapaneseEngine, "korean": KoreanEngine,
            }
            self.engines[mode] = factories[mode]()
        return self.engines[mode]

    def candidates(self, mode, value):
        if not value: return []
        if mode in ("simplified", "traditional"):
            return self.engine(mode).candidates(value, traditional=mode == "traditional")
        return self.engine(mode).candidates(value)


class MinKeyboard(tk.Tk):
    def __init__(self):
        super().__init__()
        self.title("Min Keyboard")
        self.geometry("760x560")
        self.minsize(560, 420)
        self.dictionary = Dictionary()
        self.hotkeys, self.enabled, model = load_config()
        self.grammar = GrammarChecker(model) if model and GrammarChecker.available() else None
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
        if self.grammar:
            tk.Button(self.mode_bar, text="✓ Check grammar", command=self.check_grammar).pack(side="right", padx=4)
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
        self.build_keyboard()
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
            for key in row:
                tk.Button(frame, text=key, command=lambda item=key: self.add_text(item), height=2).pack(side="left", fill="x", expand=True, padx=1, pady=1)
        bottom = tk.Frame(self.keyboard); bottom.pack(fill="x")
        space_labels = {"simplified": "空格", "traditional": "空格", "japanese": "スペース", "korean": "스페이스", "malay": "Bahasa Melayu"}
        enter_labels = {"simplified": "换行", "traditional": "換行", "japanese": "改行", "korean": "줄바꿈"}
        space_label = space_labels.get(self.mode, "Space")
        enter_label = enter_labels.get(self.mode, "Enter")
        for label, action in (("⌫", self.backspace), (space_label, self.space), (enter_label, self.enter)):
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
        self.text.insert("insert", " " if self.mode == "english" else "\u3000")

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

    def check_grammar(self):
        """Proofreads the selection, or the current paragraph, and offers the result for review."""
        if self.text.tag_ranges("sel"):
            start, end = "sel.first", "sel.last"
        else:
            start, end = "insert linestart", "insert lineend"
        start, end = self.text.index(start), self.text.index(end)
        original = self.text.get(start, end)
        if not original.strip():
            self.status.configure(text="Type or select a sentence first")
            return
        language = detect_language(original, self.mode == "malay")
        self.status.configure(text=f"Checking {language}…")

        def work():
            corrected, error = self.grammar.check(original, language)
            self.after(0, lambda: self.show_grammar(start, end, original, corrected, error))
        threading.Thread(target=work, daemon=True).start()

    def show_grammar(self, start, end, original, corrected, error):
        self.status.configure(text=f"{LABELS[self.mode]}  {self.mode}")
        dialog = tk.Toplevel(self)
        dialog.title("Grammar check")
        if error:
            tk.Label(dialog, text=error, wraplength=420, justify="left").pack(padx=12, pady=12)
            tk.Button(dialog, text="Close", command=dialog.destroy).pack(pady=(0, 12))
            return
        changed = corrected.strip() != original.strip()
        tk.Label(dialog, text="Suggested correction (review before replacing):" if changed else "No mistakes found ✓",
                 anchor="w").pack(fill="x", padx=12, pady=(12, 4))
        box = tk.Text(dialog, wrap="word", height=6, width=60, font=("sans", 13))
        box.insert("1.0", corrected)
        box.pack(fill="both", expand=True, padx=12)
        buttons = tk.Frame(dialog)
        buttons.pack(fill="x", padx=12, pady=12)

        def replace():
            if self.text.get(start, end) == original:
                self.text.delete(start, end)
                self.text.insert(start, box.get("1.0", "end-1c"))
            dialog.destroy()
        tk.Button(buttons, text="Close", command=dialog.destroy).pack(side="right")
        if changed:
            tk.Button(buttons, text="Replace", command=replace).pack(side="right", padx=6)
        # Free the model's memory after a minute without checks.
        if getattr(self, "_release_job", None):
            self.after_cancel(self._release_job)
        self._release_job = self.after(60000, self.grammar.release)


if __name__ == "__main__":
    MinKeyboard().mainloop()
