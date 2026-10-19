package org.convoy.pinyinime;

import java.util.ArrayList;
import java.util.Collections;
import java.util.HashMap;
import java.util.LinkedHashSet;
import java.util.List;
import java.util.Map;
import java.util.NavigableMap;
import java.util.TreeMap;

/** Romaji to kana conversion with kanji candidates from an SKK-derived lexicon. */
public final class JapaneseEngine {
    private static final int MAX_CANDIDATES = 40;
    private static final int MAX_COMPLETIONS = 16;

    private static final class Entry {
        final List<String> candidates;
        final int score;

        Entry(List<String> candidates, int score) {
            this.candidates = candidates;
            this.score = score;
        }
    }

    private final NavigableMap<String, Entry> lexicon = new TreeMap<>();
    private final Map<String, List<String>> shortcuts = new HashMap<>();

    public JapaneseEngine(AssetOpener assets) {
        AssetOpener.forEachLine(assets, "japanese_lexicon.tsv", line -> {
            String[] parts = line.split("\t", -1);
            if (parts.length < 2 || parts[0].isEmpty()) return;
            List<String> candidates = new ArrayList<>();
            for (String candidate : parts[1].split("\\|")) {
                if (!candidate.isEmpty()) candidates.add(candidate);
            }
            int score = parts.length > 2 && !parts[2].isEmpty() ? Integer.parseInt(parts[2]) : 0;
            lexicon.put(parts[0], new Entry(candidates, score));
        });
        AssetOpener.forEachLineIfPresent(assets, "japanese_shortcuts.tsv", line -> {
            String[] parts = line.split("\t", -1);
            if (parts.length >= 2 && !parts[0].isEmpty()) {
                shortcuts.computeIfAbsent(parts[0].toLowerCase(), ignored -> new ArrayList<>()).add(parts[1]);
            }
        });
    }

    public List<String> getCandidates(String rawInput) {
        String input = rawInput.toLowerCase();
        if (input.isEmpty()) return Collections.emptyList();
        String hiragana = toHiragana(input, false);
        String finished = toHiragana(input, true);
        LinkedHashSet<String> results = new LinkedHashSet<>();
        List<String> shortcut = shortcuts.get(input);
        if (shortcut != null) results.addAll(shortcut);
        Entry exact = lexicon.get(finished);
        if (exact != null) results.addAll(exact.candidates);
        results.add(finished);
        results.add(toKatakana(finished));

        // Complete from the kana typed so far (ignoring a trailing unfinished consonant).
        String prefix = stripTrailingLatin(hiragana);
        if (!prefix.isEmpty()) {
            List<Map.Entry<String, Entry>> matches = new ArrayList<>();
            for (Map.Entry<String, Entry> entry : lexicon.tailMap(prefix, false).entrySet()) {
                if (!entry.getKey().startsWith(prefix)) break;
                if (!entry.getKey().equals(finished)) matches.add(entry);
                if (matches.size() > 2000) break;
            }
            matches.sort((a, b) -> b.getValue().score != a.getValue().score
                    ? Integer.compare(b.getValue().score, a.getValue().score)
                    : Integer.compare(a.getKey().length(), b.getKey().length()));
            for (int i = 0; i < matches.size() && i < MAX_COMPLETIONS; i++) {
                Entry entry = matches.get(i).getValue();
                results.add(entry.candidates.isEmpty() ? matches.get(i).getKey() : entry.candidates.get(0));
            }
        }
        return new ArrayList<>(results).subList(0, Math.min(MAX_CANDIDATES, results.size()));
    }

    // ------------------------------------------------------------------ romaji ----

    private static final Map<String, String> ROMAJI = new HashMap<>();
    private static final int LONGEST_ROMAJI = 4;

    static {
        String[][] table = {
                {"a", "あ"}, {"i", "い"}, {"u", "う"}, {"e", "え"}, {"o", "お"},
                {"ka", "か"}, {"ki", "き"}, {"ku", "く"}, {"ke", "け"}, {"ko", "こ"},
                {"ga", "が"}, {"gi", "ぎ"}, {"gu", "ぐ"}, {"ge", "げ"}, {"go", "ご"},
                {"sa", "さ"}, {"si", "し"}, {"shi", "し"}, {"su", "す"}, {"se", "せ"}, {"so", "そ"},
                {"za", "ざ"}, {"zi", "じ"}, {"ji", "じ"}, {"zu", "ず"}, {"ze", "ぜ"}, {"zo", "ぞ"},
                {"ta", "た"}, {"ti", "ち"}, {"chi", "ち"}, {"tu", "つ"}, {"tsu", "つ"}, {"te", "て"}, {"to", "と"},
                {"da", "だ"}, {"di", "ぢ"}, {"du", "づ"}, {"de", "で"}, {"do", "ど"},
                {"na", "な"}, {"ni", "に"}, {"nu", "ぬ"}, {"ne", "ね"}, {"no", "の"},
                {"ha", "は"}, {"hi", "ひ"}, {"hu", "ふ"}, {"fu", "ふ"}, {"he", "へ"}, {"ho", "ほ"},
                {"ba", "ば"}, {"bi", "び"}, {"bu", "ぶ"}, {"be", "べ"}, {"bo", "ぼ"},
                {"pa", "ぱ"}, {"pi", "ぴ"}, {"pu", "ぷ"}, {"pe", "ぺ"}, {"po", "ぽ"},
                {"ma", "ま"}, {"mi", "み"}, {"mu", "む"}, {"me", "め"}, {"mo", "も"},
                {"ya", "や"}, {"yu", "ゆ"}, {"yo", "よ"}, {"ye", "いぇ"},
                {"ra", "ら"}, {"ri", "り"}, {"ru", "る"}, {"re", "れ"}, {"ro", "ろ"},
                {"la", "ら"}, {"li", "り"}, {"lu", "る"}, {"le", "れ"}, {"lo", "ろ"},
                {"wa", "わ"}, {"wi", "うぃ"}, {"we", "うぇ"}, {"wo", "を"}, {"nn", "ん"}, {"n'", "ん"}, {"xn", "ん"},
                {"va", "ゔぁ"}, {"vi", "ゔぃ"}, {"vu", "ゔ"}, {"ve", "ゔぇ"}, {"vo", "ゔぉ"},
                {"fa", "ふぁ"}, {"fi", "ふぃ"}, {"fe", "ふぇ"}, {"fo", "ふぉ"},
                {"ja", "じゃ"}, {"ju", "じゅ"}, {"je", "じぇ"}, {"jo", "じょ"},
                {"sha", "しゃ"}, {"shu", "しゅ"}, {"she", "しぇ"}, {"sho", "しょ"},
                {"cha", "ちゃ"}, {"chu", "ちゅ"}, {"che", "ちぇ"}, {"cho", "ちょ"},
                {"tsa", "つぁ"}, {"thi", "てぃ"}, {"dhi", "でぃ"}, {"twu", "とぅ"}, {"dwu", "どぅ"},
                {"xa", "ぁ"}, {"xi", "ぃ"}, {"xu", "ぅ"}, {"xe", "ぇ"}, {"xo", "ぉ"},
                {"xya", "ゃ"}, {"xyu", "ゅ"}, {"xyo", "ょ"}, {"xtu", "っ"}, {"xtsu", "っ"}, {"xwa", "ゎ"},
                {"ltu", "っ"}, {"lya", "ゃ"}, {"lyu", "ゅ"}, {"lyo", "ょ"},
                {"-", "ー"}, {",", "、"}, {".", "。"},
        };
        for (String[] pair : table) ROMAJI.put(pair[0], pair[1]);
        // Contracted sounds: kya, gya, sya, zya, tya, dya, nya, hya, bya, pya, mya, rya.
        String[][] contracted = {{"k", "き"}, {"g", "ぎ"}, {"s", "し"}, {"z", "じ"}, {"t", "ち"}, {"c", "ち"}, {"d", "ぢ"},
                {"n", "に"}, {"h", "ひ"}, {"b", "び"}, {"p", "ぴ"}, {"m", "み"}, {"r", "り"}, {"j", "じ"}};
        String[][] smallY = {{"ya", "ゃ"}, {"yu", "ゅ"}, {"ye", "ぇ"}, {"yo", "ょ"}};
        for (String[] consonant : contracted) {
            for (String[] y : smallY) ROMAJI.putIfAbsent(consonant[0] + y[0], consonant[1] + y[1]);
        }
    }

    /**
     * Converts romaji to hiragana. With {@code finish}, a trailing "n" becomes ん; otherwise
     * unconverted letters are left as typed so the caller can tell the input is unfinished.
     */
    static String toHiragana(String romaji, boolean finish) {
        StringBuilder out = new StringBuilder();
        int i = 0;
        while (i < romaji.length()) {
            char c = romaji.charAt(i);
            // Doubled consonant -> small tsu ("kitte").
            if (i + 1 < romaji.length() && c == romaji.charAt(i + 1) && isConsonant(c) && c != 'n') {
                out.append('っ');
                i++;
                continue;
            }
            // "n" before a consonant (other than y) or apostrophe -> ん.
            if (c == 'n' && i + 1 < romaji.length()) {
                char next = romaji.charAt(i + 1);
                if (next == 'n') {
                    // "konnichiha": the second n starts the next syllable; "honn": both make one ん.
                    char after = i + 2 < romaji.length() ? romaji.charAt(i + 2) : 0;
                    out.append('ん');
                    i += "aeiouy".indexOf(after) >= 0 && after != 0 ? 1 : 2;
                    continue;
                }
                if (next == '\'') {
                    out.append('ん');
                    i += 2;
                    continue;
                }
                if (isConsonant(next) && next != 'y') {
                    out.append('ん');
                    i++;
                    continue;
                }
            }
            String match = null;
            int length = 0;
            for (int len = Math.min(LONGEST_ROMAJI, romaji.length() - i); len >= 1; len--) {
                String piece = ROMAJI.get(romaji.substring(i, i + len));
                if (piece != null) {
                    match = piece;
                    length = len;
                    break;
                }
            }
            if (match != null) {
                out.append(match);
                i += length;
            } else {
                out.append(c);
                i++;
            }
        }
        if (finish && out.length() > 0 && out.charAt(out.length() - 1) == 'n') {
            out.setCharAt(out.length() - 1, 'ん');
        }
        return out.toString();
    }

    static String toKatakana(String hiragana) {
        StringBuilder out = new StringBuilder(hiragana.length());
        for (int i = 0; i < hiragana.length(); i++) {
            char c = hiragana.charAt(i);
            out.append(c >= 'ぁ' && c <= 'ゖ' ? (char) (c + 0x60) : c);
        }
        return out.toString();
    }

    private static String stripTrailingLatin(String text) {
        int end = text.length();
        while (end > 0 && text.charAt(end - 1) < 0x80) end--;
        return text.substring(0, end);
    }

    private static boolean isConsonant(char c) {
        return c >= 'a' && c <= 'z' && "aeiou".indexOf(c) < 0;
    }
}
