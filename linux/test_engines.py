"""Run with: python3 -m unittest discover -s linux"""
import unittest

from engines import JapaneseEngine, KoreanEngine, LatinEngine, PinyinEngine, to_hiragana
from grammar import detect_language


class EnginesTest(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.english = LatinEngine("english_words.txt", "english_shortcuts.tsv")
        cls.malay = LatinEngine("malay_words.txt")
        cls.pinyin = PinyinEngine()
        cls.japanese = JapaneseEngine()
        cls.korean = KoreanEngine()

    def test_english(self):
        self.assertEqual(self.english.candidates("the")[0], "the")
        self.assertIn("the", self.english.candidates("teh")[:3])
        self.assertIn("what do you mean", self.english.candidates("wdym"))

    def test_malay(self):
        self.assertIn("terima", self.malay.candidates("teri"))

    def test_pinyin(self):
        self.assertEqual(self.pinyin.candidates("wo")[0], "我")
        self.assertEqual(self.pinyin.candidates("woaini")[0], "我爱你")
        self.assertEqual(self.pinyin.candidates("woaini", traditional=True)[0], "我愛你")

    def test_japanese(self):
        self.assertEqual(to_hiragana("konnichiha"), "こんにちは")
        self.assertEqual(to_hiragana("kitte"), "きって")
        self.assertEqual(self.japanese.candidates("watashi")[0], "私")

    def test_korean(self):
        self.assertEqual(self.korean.candidates("annyeonghaseyo")[0], "안녕하세요")

    def test_language_detection(self):
        self.assertEqual(detect_language("我昨天去了商店"), "Mandarin Chinese")
        self.assertEqual(detect_language("Saya tidak suka makan nasi"), "Malay")
        self.assertEqual(detect_language("She go to school"), "English")


if __name__ == "__main__":
    unittest.main()
