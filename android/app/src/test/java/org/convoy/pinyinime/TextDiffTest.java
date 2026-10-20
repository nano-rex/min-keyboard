package org.convoy.pinyinime;

import static org.junit.Assert.assertEquals;
import static org.junit.Assert.assertFalse;
import static org.junit.Assert.assertTrue;

import org.junit.Test;

public class TextDiffTest {
    @Test
    public void marksChangedWords() {
        assertEquals("[[She ], -[go], +[went], [ to school]]", TextDiff.diff("She go to school", "She went to school").toString());
    }

    @Test
    public void diffsChineseByCharacter() {
        String parts = TextDiff.diff("我买了很多的东西了。", "我买了很多东西。").toString();
        assertEquals("[[我买了很多], -[的], [东西], -[了], [。]]", parts);
    }

    @Test
    public void detectsNoChange() {
        assertFalse(TextDiff.changed("All good. ", "All good."));
        assertTrue(TextDiff.changed("teh cat", "the cat"));
    }
}
