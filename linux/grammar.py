"""Optional on-device grammar check for the Linux edition.

Uses llama-cpp-python (`pip install llama-cpp-python`) with the same GGUF models
and prompts as the Android app (android/app/src/main/cpp/grammar_core.cpp).
Everything here is optional: without the package or a model the app works as before.
"""

import os
from pathlib import Path

EXAMPLES = {
    "English": ("he dont know where there going tomorow, me and her was waiting",
                "He doesn't know where they're going tomorrow. She and I were waiting."),
    "Mandarin Chinese": ("我昨天去了图书馆看书了，因为我明天有考试的。", "我昨天去图书馆看书了，因为我明天有考试。"),
    "Malay": ("Saya ada tiga kucing-kucing dan dia suka makan ikan sangat.",
              "Saya ada tiga ekor kucing dan dia sangat suka makan ikan."),
}

MALAY_WORDS = set("yang dan saya tidak tak ini itu untuk dengan akan ada di ke dari kita kami mereka sudah belum "
                  "boleh dia apa nak aku awak kamu pada juga atau telah sangat lagi mahu hendak kerana tetapi".split())
ENGLISH_WORDS = set("the and is are was were to of in that it you i he she we they this have has with for not be on at my your will".split())


def detect_language(text, malay_mode=False):
    han = sum(1 for c in text if "一" <= c <= "鿿")
    letters = sum(1 for c in text if c.isalpha()) - han
    if han and han * 2 >= letters / 4:
        return "Mandarin Chinese"
    words = [w.strip(".,!?;:\"'()").lower() for w in text.split()]
    malay = sum(w in MALAY_WORDS for w in words)
    english = sum(w in ENGLISH_WORDS for w in words)
    if malay != english:
        return "Malay" if malay > english else "English"
    return "Malay" if malay_mode else "English"


def system_prompt(language):
    return (f"You are a careful proofreader for {language}. "
            "Find and fix every grammar, spelling, punctuation and word-choice mistake in the text the user gives you "
            "(for example verb tenses, agreement, wrong words that sound alike, missing words, measure words and word order) "
            f"so that it is correct, natural {language}. "
            "Keep the original meaning, tone, names, line breaks and writing system "
            "(Chinese written in Traditional characters stays Traditional, Simplified stays Simplified). "
            "Do not translate, do not answer questions in the text, do not add anything, and do not explain. "
            "If the text is already correct, return it unchanged. Reply with the corrected text only.")


def memory_problem(model_path):
    """Same budget as Android: at most 1/4 of RAM and 1/2 of what is free. None if it fits."""
    try:
        meminfo = dict(line.split(":", 1) for line in Path("/proc/meminfo").read_text().splitlines())
        total = int(meminfo["MemTotal"].split()[0]) * 1024
        available = int(meminfo["MemAvailable"].split()[0]) * 1024
    except (OSError, KeyError, ValueError):
        return None
    needed = os.path.getsize(model_path) + 300 * 1024 * 1024
    if needed > total * 0.25:
        return f"This model needs about {needed >> 20} MB, more than a quarter of this computer's RAM."
    if needed > available * 0.5:
        return f"Not enough free memory right now ({available >> 20} MB free, {needed >> 20} MB needed). Close some programs."
    return None


class GrammarChecker:
    def __init__(self, model_path):
        self.model_path = model_path
        self.llm = None

    @staticmethod
    def available():
        try:
            import llama_cpp  # noqa: F401
            return True
        except ImportError:
            return False

    def check(self, text, language):
        """Returns (corrected_text, None) or (None, error message)."""
        if not self.model_path or not Path(self.model_path).is_file():
            return None, "Set [ai] model in ~/.config/min-keyboard/config.ini to a .gguf file."
        if self.llm is None:
            problem = memory_problem(self.model_path)
            if problem:
                return None, problem
            from llama_cpp import Llama
            self.llm = Llama(model_path=self.model_path, n_ctx=1024, n_threads=max(1, (os.cpu_count() or 2) // 2),
                             use_mmap=True, use_mlock=False, verbose=False)
        wrong, right = EXAMPLES.get(language, EXAMPLES["English"])
        ask = f"Proofread this {language} text:\n\n"
        messages = [
            {"role": "system", "content": system_prompt(language)},
            {"role": "user", "content": ask + wrong + " /no_think"},
            {"role": "assistant", "content": right},
            {"role": "user", "content": ask + text + " /no_think"},
        ]
        reply = self.llm.create_chat_completion(messages=messages, temperature=0, max_tokens=512)
        answer = reply["choices"][0]["message"]["content"] or ""
        if "</think>" in answer:
            answer = answer.split("</think>", 1)[1]
        return answer.strip().strip("`").strip(), None

    def release(self):
        self.llm = None
