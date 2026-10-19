package org.convoy.pinyinime;

import java.util.Arrays;
import java.util.HashSet;
import java.util.Locale;
import java.util.Set;

/** Picks the language to proofread in: English, Mandarin Chinese or Malay. */
final class GrammarLanguage {
    static final String ENGLISH = "English";
    static final String MANDARIN = "Mandarin Chinese";
    static final String MALAY = "Malay";

    private static final Set<String> MALAY_WORDS = new HashSet<>(Arrays.asList(
            "yang", "dan", "saya", "tidak", "tak", "ini", "itu", "untuk", "dengan", "akan", "ada", "di", "ke",
            "dari", "kita", "kami", "mereka", "sudah", "belum", "boleh", "dia", "apa", "nak", "aku", "awak",
            "kamu", "pada", "juga", "atau", "telah", "sangat", "lagi", "mahu", "hendak", "kerana", "tetapi",
            "bagaimana", "mengapa", "kenapa", "bila", "sini", "sana", "mana", "semua", "satu", "orang", "baik",
            "terima", "kasih", "selamat", "makan", "pergi", "datang", "rumah", "sekolah", "kerja", "hari"));
    private static final Set<String> ENGLISH_WORDS = new HashSet<>(Arrays.asList(
            "the", "and", "is", "are", "was", "were", "to", "of", "in", "that", "it", "you", "i", "he", "she",
            "we", "they", "this", "have", "has", "with", "for", "not", "be", "on", "at", "my", "your", "will"));

    private GrammarLanguage() {
    }

    /**
     * Chinese characters mean Mandarin. Otherwise common function words decide between Malay and
     * English, with the keyboard's current mode as the tie-breaker.
     */
    static String detect(String text, boolean malayMode) {
        int han = 0, letters = 0;
        for (int i = 0; i < text.length(); i++) {
            char c = text.charAt(i);
            if (c >= '一' && c <= '鿿') han++;
            else if (Character.isLetter(c)) letters++;
        }
        if (han > 0 && han * 2 >= letters / 4) return MANDARIN;
        int malay = 0, english = 0;
        for (String word : text.toLowerCase(Locale.ROOT).split("[^a-z'-]+")) {
            if (MALAY_WORDS.contains(word)) malay++;
            if (ENGLISH_WORDS.contains(word)) english++;
        }
        if (malay > english) return MALAY;
        if (english > malay) return ENGLISH;
        return malayMode ? MALAY : ENGLISH;
    }
}
