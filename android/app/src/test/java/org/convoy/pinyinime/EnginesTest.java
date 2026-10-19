package org.convoy.pinyinime;

import static org.junit.Assert.assertEquals;
import static org.junit.Assert.assertNull;
import static org.junit.Assert.assertTrue;

import java.io.FileInputStream;
import java.io.File;
import java.util.List;

import org.junit.BeforeClass;
import org.junit.Test;

/** Runs the input engines against the real bundled assets. */
public class EnginesTest {
    private static final AssetOpener ASSETS = name -> new FileInputStream(new File("src/main/assets", name));

    private static LatinEngine english;
    private static LatinEngine malay;
    private static PinyinEngine pinyin;
    private static JapaneseEngine japanese;
    private static KoreanEngine korean;

    @BeforeClass
    public static void load() {
        english = LatinEngine.english(ASSETS);
        malay = LatinEngine.malay(ASSETS);
        pinyin = new PinyinEngine(ASSETS);
        japanese = new JapaneseEngine(ASSETS);
        korean = new KoreanEngine(ASSETS);
    }

    private static void assertFirst(String expected, List<String> candidates) {
        assertTrue("no candidates, expected " + expected, !candidates.isEmpty());
        assertEquals(candidates.toString(), expected, candidates.get(0));
    }

    private static void assertContains(String expected, List<String> candidates) {
        assertTrue(expected + " missing from " + candidates, candidates.contains(expected));
    }

    @Test
    public void englishCompletesByFrequency() {
        assertFirst("the", english.getCandidates("the"));
        assertContains("there", english.getCandidates("the"));
        assertContains("tomorrow", english.getCandidates("tomo"));
        assertContains("definitely", english.getCandidates("defin"));
    }

    @Test
    public void englishCorrectsTypos() {
        assertFirst("the", english.getCandidates("teh"));
        assertContains("receive", english.getCandidates("recieve"));
        assertEquals("the", english.autoCorrection("teh"));
        assertEquals("receive", english.autoCorrection("recieve"));
        assertEquals("definitely", english.autoCorrection("definately"));
    }

    @Test
    public void englishAutocorrectLeavesRealWordsAlone() {
        assertNull(english.autoCorrection("were"));
        assertNull(english.autoCorrection("form"));
        assertNull(english.autoCorrection("hel"));
        assertNull(english.autoCorrection("cat"));
    }

    @Test
    public void englishFixesContractionsAndCapitals() {
        assertEquals("don't", english.autoCorrection("dont"));
        assertEquals("I'm", english.autoCorrection("im"));
        assertEquals("I", english.autoCorrection("i"));
        assertEquals("Monday", english.autoCorrection("monday"));
        assertFirst("I'm", english.getCandidates("im"));
    }

    @Test
    public void malayWords() {
        assertFirst("saya", malay.getCandidates("saya"));
        assertContains("terima", malay.getCandidates("teri"));
        assertContains("bagaimana", malay.getCandidates("bagai"));
        assertEquals("bagaimana", malay.autoCorrection("bagaimna"));
        assertNull(malay.autoCorrection("makan"));
    }

    @Test
    public void pinyinRanksCommonCharactersFirst() {
        assertFirst("我", pinyin.getCandidates("wo", PinyinEngine.ScriptMode.SIMPLIFIED));
        assertFirst("你", pinyin.getCandidates("ni", PinyinEngine.ScriptMode.SIMPLIFIED));
        assertFirst("是", pinyin.getCandidates("shi", PinyinEngine.ScriptMode.SIMPLIFIED));
        assertFirst("的", pinyin.getCandidates("de", PinyinEngine.ScriptMode.SIMPLIFIED));
        assertFirst("謝謝", pinyin.getCandidates("xiexie", PinyinEngine.ScriptMode.TRADITIONAL));
    }

    @Test
    public void pinyinSegmentsSentences() {
        assertFirst("我爱你", pinyin.getCandidates("woaini", PinyinEngine.ScriptMode.SIMPLIFIED));
        assertFirst("今天天气很好", pinyin.getCandidates("jintiantianqihenhao", PinyinEngine.ScriptMode.SIMPLIFIED));
        assertFirst("我愛你", pinyin.getCandidates("woaini", PinyinEngine.ScriptMode.TRADITIONAL));
        assertFirst("你好", pinyin.getCandidates("nihao", PinyinEngine.ScriptMode.SIMPLIFIED));
    }

    @Test
    public void romajiToKana() {
        assertEquals("こんにちは", JapaneseEngine.toHiragana("konnichiha", true));
        assertEquals("きって", JapaneseEngine.toHiragana("kitte", true));
        assertEquals("ほん", JapaneseEngine.toHiragana("hon", true));
        assertEquals("ほん", JapaneseEngine.toHiragana("honn", true));
        assertEquals("しんよう", JapaneseEngine.toHiragana("shin'you", true));
        assertEquals("きゃく", JapaneseEngine.toHiragana("kyaku", true));
        assertEquals("ちゃ", JapaneseEngine.toHiragana("cha", true));
        assertEquals("カタカナ", JapaneseEngine.toKatakana("かたかな"));
    }

    @Test
    public void japaneseConvertsToKanji() {
        assertFirst("私", japanese.getCandidates("watashi"));
        assertFirst("日本", japanese.getCandidates("nihon"));
        assertContains("にほん", japanese.getCandidates("nihon"));
        assertContains("ニホン", japanese.getCandidates("nihon"));
        assertContains("記者", japanese.getCandidates("kisha"));
    }

    @Test
    public void koreanWordsAndSyllables() {
        assertFirst("안녕하세요", korean.getCandidates("annyeonghaseyo"));
        assertFirst("감사합니다", korean.getCandidates("gamsahamnida"));
        assertContains("사랑해", korean.getCandidates("saranghae"));
        assertEquals("한국", KoreanEngine.compose("hanguk"));
        assertEquals("사랑해", KoreanEngine.compose("saranghae"));
        assertContains("한국어", korean.getCandidates("hangug"));
    }
}
