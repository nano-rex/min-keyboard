package org.convoy.pinyinime;

import java.util.ArrayList;
import java.util.Arrays;
import java.util.Collections;
import java.util.HashMap;
import java.util.LinkedHashSet;
import java.util.List;
import java.util.Map;
import java.util.NavigableMap;
import java.util.TreeMap;

/**
 * Romanized Korean input. Words come from a lexicon keyed by Revised Romanization; anything
 * else is composed syllable by syllable, so every input produces Hangul.
 */
public final class KoreanEngine {
    private static final int MAX_CANDIDATES = 32;
    private static final int MAX_COMPLETIONS = 20;

    private static final String[] INITIALS = {"g", "kk", "n", "d", "tt", "r", "m", "b", "pp", "s", "ss", "", "j", "jj", "ch", "k", "t", "p", "h"};
    private static final String[] MEDIALS = {"a", "ae", "ya", "yae", "eo", "e", "yeo", "ye", "o", "wa", "wae", "oe", "yo", "u", "wo", "we", "wi", "yu", "eu", "ui", "i"};
    // Typed spellings for each final consonant index (0 = none). Only simple finals are typed.
    private static final String[][] FINALS = {
            {}, {"k", "g"}, {"kk"}, {}, {"n"}, {}, {}, {"t", "d"}, {"l", "r"}, {}, {}, {}, {}, {}, {}, {},
            {"m"}, {"p", "b"}, {}, {"s"}, {"ss"}, {"ng"}, {"j"}, {"ch"}, {}, {}, {}, {"h"},
    };
    private static final int LONGEST_SYLLABLE = 8;

    private static final class Entry {
        final List<String> words;
        final int score;

        Entry(List<String> words, int score) {
            this.words = words;
            this.score = score;
        }
    }

    private final NavigableMap<String, Entry> lexicon = new TreeMap<>();
    private final Map<String, List<String>> shortcuts = new HashMap<>();
    private static final Map<String, Character> SYLLABLES = buildSyllables();

    public KoreanEngine(AssetOpener assets) {
        AssetOpener.forEachLine(assets, "korean_lexicon.tsv", line -> {
            String[] parts = line.split("\t", -1);
            if (parts.length < 2 || parts[0].isEmpty()) return;
            int score = parts.length > 2 && !parts[2].isEmpty() ? Integer.parseInt(parts[2]) : 0;
            lexicon.put(parts[0], new Entry(Arrays.asList(parts[1].split("\\|")), score));
        });
        AssetOpener.forEachLineIfPresent(assets, "korean_shortcuts.tsv", line -> {
            String[] parts = line.split("\t", -1);
            if (parts.length >= 2 && !parts[0].isEmpty()) {
                shortcuts.computeIfAbsent(parts[0].toLowerCase(), ignored -> new ArrayList<>()).add(parts[1]);
            }
        });
    }

    public List<String> getCandidates(String rawInput) {
        String input = rawInput.toLowerCase();
        if (input.isEmpty()) return Collections.emptyList();
        LinkedHashSet<String> results = new LinkedHashSet<>();
        Entry exact = lexicon.get(input);
        if (exact != null) results.addAll(exact.words);
        List<String> shortcut = shortcuts.get(input);
        if (shortcut != null) results.addAll(shortcut);
        String composed = compose(input);
        if (composed != null) results.add(composed);

        List<Map.Entry<String, Entry>> matches = new ArrayList<>();
        for (Map.Entry<String, Entry> entry : lexicon.tailMap(input, false).entrySet()) {
            if (!entry.getKey().startsWith(input)) break;
            matches.add(entry);
            if (matches.size() > 3000) break;
        }
        matches.sort((a, b) -> Integer.compare(b.getValue().score, a.getValue().score));
        for (int i = 0; i < matches.size() && i < MAX_COMPLETIONS; i++) {
            results.addAll(matches.get(i).getValue().words.subList(0, 1));
        }
        if (results.isEmpty()) results.add(rawInput);
        return new ArrayList<>(results).subList(0, Math.min(MAX_CANDIDATES, results.size()));
    }

    /**
     * Splits romanized input into Hangul syllables, preferring fewer syllables and syllables that
     * start with a consonant ("hanguk" = 한국, not 항욱). Returns null if the input cannot be parsed.
     */
    static String compose(String input) {
        int n = input.length();
        int[] cost = new int[n + 1];
        int[] previous = new int[n + 1];
        char[] syllable = new char[n + 1];
        Arrays.fill(cost, Integer.MAX_VALUE);
        cost[0] = 0;
        for (int i = 0; i < n; i++) {
            if (cost[i] == Integer.MAX_VALUE) continue;
            for (int j = i + 1; j <= Math.min(n, i + LONGEST_SYLLABLE); j++) {
                Character hangul = SYLLABLES.get(input.substring(i, j));
                if (hangul == null) continue;
                boolean startsWithVowel = "aeiouwy".indexOf(input.charAt(i)) >= 0;
                int next = cost[i] + 10 + (startsWithVowel && i > 0 ? 1 : 0);
                if (next < cost[j]) {
                    cost[j] = next;
                    previous[j] = i;
                    syllable[j] = hangul;
                }
            }
        }
        if (cost[n] == Integer.MAX_VALUE) return null;
        StringBuilder out = new StringBuilder();
        for (int at = n; at > 0; at = previous[at]) out.append(syllable[at]);
        return out.reverse().toString();
    }

    private static Map<String, Character> buildSyllables() {
        Map<String, Character> map = new HashMap<>();
        for (int initial = 0; initial < INITIALS.length; initial++) {
            for (int medial = 0; medial < MEDIALS.length; medial++) {
                String open = INITIALS[initial] + MEDIALS[medial];
                map.putIfAbsent(open, (char) (0xAC00 + (initial * 21 + medial) * 28));
                for (int fin = 1; fin < FINALS.length; fin++) {
                    for (String spelling : FINALS[fin]) {
                        map.putIfAbsent(open + spelling, (char) (0xAC00 + (initial * 21 + medial) * 28 + fin));
                    }
                }
            }
        }
        // "l" is also typed as an initial ㄹ (e.g. loanwords).
        for (int medial = 0; medial < MEDIALS.length; medial++) {
            map.putIfAbsent("l" + MEDIALS[medial], (char) (0xAC00 + (5 * 21 + medial) * 28));
        }
        return map;
    }
}
