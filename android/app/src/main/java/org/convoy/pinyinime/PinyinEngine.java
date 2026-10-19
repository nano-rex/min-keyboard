package org.convoy.pinyinime;

import java.util.ArrayList;
import java.util.Collections;
import java.util.LinkedHashSet;
import java.util.List;
import java.util.Map;
import java.util.NavigableMap;
import java.util.TreeMap;

/**
 * Pinyin to Chinese conversion. Lexicon candidates are stored most frequent first. Input that is
 * not a single dictionary entry ("woaini") is segmented into the fewest dictionary words.
 */
public class PinyinEngine {
    public enum ScriptMode {
        SIMPLIFIED,
        TRADITIONAL
    }

    private static final String LEXICON_ASSET = "pinyin_lexicon.tsv";
    private static final String SHORTCUTS_ASSET = "pinyin_shortcuts.tsv";
    private static final int MAX_CANDIDATES = 64;
    private static final int MAX_KEY_LENGTH = 30;
    /** Roughly the token count behind the lexicon frequencies (jieba's dictionary totals ~6e7). */
    private static final double LOG_CORPUS_SIZE = Math.log(6e7);

    private static final class Entry {
        final List<String> simplified;
        final List<String> traditional;
        /** Segmentation cost of the top candidate: -log(frequency / corpus size). */
        final double cost;

        Entry(List<String> simplified, List<String> traditional, long frequency) {
            this.simplified = simplified;
            this.traditional = traditional;
            this.cost = LOG_CORPUS_SIZE - Math.log(frequency + 1.0);
        }

        List<String> candidates(ScriptMode mode) {
            return mode == ScriptMode.TRADITIONAL ? traditional : simplified;
        }
    }

    private final NavigableMap<String, Entry> lexicon = new TreeMap<>();
    private final NavigableMap<String, Entry> shortcuts = new TreeMap<>();

    public PinyinEngine(AssetOpener assets) {
        addFallbackShortcuts();
        AssetOpener.forEachLine(assets, SHORTCUTS_ASSET, line -> {
            String[] parts = line.split("\t", -1);
            if (parts.length >= 3 && !normalize(parts[0]).isEmpty()) {
                shortcuts.put(normalize(parts[0]), new Entry(splitCandidates(parts[1]), splitCandidates(parts[2]), 0));
            }
        });
        AssetOpener.forEachLine(assets, LEXICON_ASSET, line -> {
            String[] parts = line.split("\t", -1);
            if (parts.length >= 3 && !normalize(parts[0]).isEmpty()) {
                long frequency = parts.length > 3 && !parts[3].isEmpty() ? Long.parseLong(parts[3]) : 0;
                lexicon.put(normalize(parts[0]), new Entry(splitCandidates(parts[1]), splitCandidates(parts[2]), frequency));
            }
        });
    }

    public boolean isComposingChar(char ch) {
        return (ch >= 'a' && ch <= 'z') || ch == '\'';
    }

    public List<String> getCandidates(String rawInput, ScriptMode scriptMode) {
        String input = normalize(rawInput);
        if (input.isEmpty()) {
            return Collections.emptyList();
        }

        LinkedHashSet<String> results = new LinkedHashSet<>();
        Entry shortcut = shortcuts.get(input.replace("'", ""));
        if (shortcut != null) {
            addAll(results, shortcut.candidates(scriptMode));
        }
        Entry exact = lexicon.get(input.replace("'", ""));
        if (exact == null || input.indexOf('\'') >= 0) {
            String sentence = sentence(input, scriptMode);
            if (sentence != null) {
                results.add(sentence);
            }
        }
        if (exact != null) {
            addAll(results, exact.candidates(scriptMode));
        }
        String compact = input.replace("'", "");
        for (Map.Entry<String, Entry> entry : lexicon.tailMap(compact, false).entrySet()) {
            if (results.size() >= MAX_CANDIDATES || !entry.getKey().startsWith(compact)) {
                break;
            }
            addAll(results, entry.getValue().candidates(scriptMode));
        }
        // Single characters for the first syllable, so long input can still be typed piece by piece.
        String first = firstWord(input, scriptMode);
        if (first != null) {
            results.add(first);
        }
        return new ArrayList<>(results).subList(0, Math.min(MAX_CANDIDATES, results.size()));
    }

    /**
     * Splits the input into the most probable sequence of lexicon words (a unigram model) and joins each key's top candidate.
     * The last piece may be an unfinished syllable, completed with the most frequent match.
     * Apostrophes force a split. Returns null when the input cannot be segmented.
     */
    String sentence(String input, ScriptMode scriptMode) {
        String[] pieces = input.split("'");
        StringBuilder result = new StringBuilder();
        int segments = 0;
        for (int p = 0; p < pieces.length; p++) {
            String piece = pieces[p];
            if (piece.isEmpty()) continue;
            Segmentation segmentation = segment(piece, scriptMode, p == pieces.length - 1);
            if (segmentation == null) return null;
            result.append(segmentation.text);
            segments += segmentation.count;
        }
        return segments >= 2 || input.indexOf('\'') >= 0 ? result.toString() : null;
    }

    private static final class Segmentation {
        final int count;
        final String text;

        Segmentation(int count, String text) {
            this.count = count;
            this.text = text;
        }
    }

    private Segmentation segment(String piece, ScriptMode scriptMode, boolean allowPrefixAtEnd) {
        int n = piece.length();
        double[] best = new double[n + 1];
        int[] count = new int[n + 1];
        String[] word = new String[n + 1];
        int[] previous = new int[n + 1];
        java.util.Arrays.fill(best, Double.MAX_VALUE);
        best[0] = 0;
        for (int i = 0; i < n; i++) {
            if (best[i] == Double.MAX_VALUE) continue;
            for (int j = i + 1; j <= Math.min(n, i + MAX_KEY_LENGTH); j++) {
                String key = piece.substring(i, j);
                Entry entry = lexicon.get(key);
                if (entry == null && j == n && allowPrefixAtEnd) {
                    entry = completion(key, scriptMode);
                }
                String candidate = top(entry, scriptMode);
                if (candidate != null && best[i] + entry.cost < best[j]) {
                    best[j] = best[i] + entry.cost;
                    count[j] = count[i] + 1;
                    word[j] = candidate;
                    previous[j] = i;
                }
            }
        }
        if (best[n] == Double.MAX_VALUE) return null;
        List<String> parts = new ArrayList<>();
        for (int at = n; at > 0; at = previous[at]) {
            parts.add(word[at]);
        }
        Collections.reverse(parts);
        return new Segmentation(count[n], String.join("", parts));
    }

    private String firstWord(String input, ScriptMode scriptMode) {
        String compact = input.replace("'", "");
        for (int j = Math.min(compact.length() - 1, 6); j >= 1; j--) {
            String candidate = top(lexicon.get(compact.substring(0, j)), scriptMode);
            if (candidate != null) return candidate;
        }
        return null;
    }

    /** The most frequent entry whose key completes the prefix, e.g. the unfinished last syllable. */
    private Entry completion(String prefix, ScriptMode scriptMode) {
        Entry best = null;
        int scanned = 0;
        for (Map.Entry<String, Entry> entry : lexicon.tailMap(prefix, false).entrySet()) {
            if (!entry.getKey().startsWith(prefix) || ++scanned > 200) break;
            if (top(entry.getValue(), scriptMode) != null && (best == null || entry.getValue().cost < best.cost)) {
                best = entry.getValue();
            }
        }
        return best;
    }

    private static String top(Entry entry, ScriptMode scriptMode) {
        if (entry == null) return null;
        List<String> candidates = entry.candidates(scriptMode);
        return candidates.isEmpty() ? null : candidates.get(0);
    }

    private static void addAll(LinkedHashSet<String> results, List<String> candidates) {
        for (String candidate : candidates) {
            if (results.size() >= MAX_CANDIDATES) return;
            if (!candidate.isEmpty()) results.add(candidate);
        }
    }

    private void addFallbackShortcuts() {
        shortcuts.put("bzd", new Entry(Collections.singletonList("不知道"), Collections.singletonList("不知道"), 0));
        shortcuts.put("wsm", new Entry(Collections.singletonList("为什么"), Collections.singletonList("為什麼"), 0));
        shortcuts.put("yyds", new Entry(Collections.singletonList("永远的神"), Collections.singletonList("永遠的神"), 0));
        shortcuts.put("xswl", new Entry(Collections.singletonList("笑死我了"), Collections.singletonList("笑死我了"), 0));
    }

    private static List<String> splitCandidates(String block) {
        if (block == null || block.isEmpty()) {
            return Collections.emptyList();
        }
        List<String> result = new ArrayList<>();
        for (String part : block.split("\\|")) {
            if (!part.isEmpty()) result.add(part);
        }
        return result;
    }

    private static String normalize(String text) {
        if (text == null || text.isEmpty()) {
            return "";
        }
        StringBuilder builder = new StringBuilder(text.length());
        for (int i = 0; i < text.length(); i++) {
            char ch = Character.toLowerCase(text.charAt(i));
            if ((ch >= 'a' && ch <= 'z') || ch == '\'') {
                builder.append(ch);
            } else if (ch == 'ü') {
                builder.append('v');
            }
        }
        return builder.toString();
    }
}
