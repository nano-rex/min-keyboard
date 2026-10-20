package org.convoy.pinyinime;

import java.util.ArrayList;
import java.util.Arrays;
import java.util.Collections;
import java.util.HashMap;
import java.util.LinkedHashMap;
import java.util.LinkedHashSet;
import java.util.List;
import java.util.Locale;
import java.util.Map;

/**
 * Word completion and spelling correction for languages written with the Latin alphabet
 * (English and Malay). Word lists are ordered most frequent first, so a word's line number is
 * its frequency rank.
 */
public final class LatinEngine {
    private static final int MAX_CANDIDATES = 32;
    private static final int MAX_CORRECTIONS = 6;
    private static final String QWERTY_ROWS = "qwertyuiop|asdfghjkl|zxcvbnm";

    private final String[] words;           // display form, by frequency rank
    private final String[] sortedKeys;      // lower-case forms, alphabetical
    private final int[] sortedRanks;        // rank of sortedKeys[i]
    private final Map<String, Integer> rankByKey = new HashMap<>();
    private final Map<Integer, List<Integer>> ranksByLength = new HashMap<>();
    private final Map<String, List<String>> aliases;

    private LatinEngine(AssetOpener assets, String wordsAsset, String shortcutsAsset, Map<String, List<String>> builtInAliases) {
        List<String> loaded = new ArrayList<>();
        AssetOpener.forEachLine(assets, wordsAsset, line -> {
            String word = line.trim();
            if (!word.isEmpty() && !rankByKey.containsKey(key(word))) {
                rankByKey.put(key(word), loaded.size());
                loaded.add(word);
            }
        });
        words = loaded.toArray(new String[0]);
        sortedKeys = rankByKey.keySet().toArray(new String[0]);
        Arrays.sort(sortedKeys);
        sortedRanks = new int[sortedKeys.length];
        for (int i = 0; i < sortedKeys.length; i++) {
            sortedRanks[i] = rankByKey.get(sortedKeys[i]);
        }
        for (int rank = 0; rank < words.length; rank++) {
            ranksByLength.computeIfAbsent(key(words[rank]).length(), ignored -> new ArrayList<>()).add(rank);
        }
        aliases = new LinkedHashMap<>(builtInAliases);
        if (shortcutsAsset != null) {
            AssetOpener.forEachLineIfPresent(assets, shortcutsAsset, line -> {
                String[] parts = line.split("\t", -1);
                if (parts.length >= 2 && !parts[0].trim().isEmpty() && !parts[1].trim().isEmpty()) {
                    aliases.put(parts[0].trim().toLowerCase(Locale.ROOT), Collections.singletonList(parts[1].trim()));
                }
            });
        }
    }

    public static LatinEngine english(AssetOpener assets) {
        return new LatinEngine(assets, "english_words.txt", "english_shortcuts.tsv", englishAliases());
    }

    public static LatinEngine malay(AssetOpener assets) {
        return new LatinEngine(assets, "malay_words.txt", "malay_shortcuts.tsv", Collections.emptyMap());
    }

    public boolean isKnownWord(String input) {
        return rankByKey.containsKey(key(input)) || aliases.containsKey(key(input));
    }

    /** Candidates for the word being typed: shortcuts, the best match, the literal input, completions and corrections. */
    public List<String> getCandidates(String rawInput) {
        String input = key(rawInput);
        if (input.isEmpty()) {
            return Collections.emptyList();
        }
        LinkedHashSet<String> results = new LinkedHashSet<>();
        List<String> alias = aliases.get(input);
        if (alias != null) {
            results.addAll(alias);
        }
        Integer exact = rankByKey.get(input);
        List<Integer> completionRanks = completionRanks(input, MAX_CANDIDATES);
        List<String> completions = new ArrayList<>();
        for (int rank : completionRanks) completions.add(words[rank]);
        List<Scored> scored = scoreCorrections(input);
        List<String> corrections = new ArrayList<>();
        for (int i = 0; i < scored.size() && i < MAX_CORRECTIONS; i++) corrections.add(words[scored.get(i).rank]);

        if (exact != null) {
            results.add(words[exact]);
        } else if (!scored.isEmpty() && !likelyUnfinished(scored.get(0), completionRanks)) {
            results.add(corrections.get(0));
        } else if (!completions.isEmpty()) {
            results.add(completions.get(0));
        }
        results.add(rawInput);
        for (String alternative : aliasCompletions(input)) {
            results.add(alternative);
        }
        // Interleave so both completions and corrections show up in the first few slots.
        int c = 0, k = 0;
        while (results.size() < MAX_CANDIDATES && (c < completions.size() || k < corrections.size())) {
            if (c < completions.size()) results.add(completions.get(c++));
            if (c < completions.size()) results.add(completions.get(c++));
            if (k < corrections.size()) results.add(corrections.get(k++));
        }
        return new ArrayList<>(results).subList(0, Math.min(MAX_CANDIDATES, results.size()));
    }

    /**
     * The word to substitute when the user finishes an unknown word, or null to leave it alone.
     * Only confident fixes are returned: the typed word must be unknown and a close, common word must exist.
     */
    public String autoCorrection(String rawInput) {
        String input = key(rawInput);
        Integer rank = rankByKey.get(input);
        if (rank != null) {
            // Known word: only fix capitalisation of words that are always capitalised ("i", "monday").
            String word = words[rank];
            return !word.equals(rawInput) && !word.equals(word.toLowerCase(Locale.ROOT)) && rawInput.equals(input) ? word : null;
        }
        List<String> alias = aliases.get(input);
        if (alias != null) {
            // Missing apostrophes: "dont" -> "don't", "im" -> "I'm". Multi-word shortcuts stay as suggestions.
            String first = alias.get(0);
            return key(first.replace("'", "")).equals(input.replace("'", "")) ? first : null;
        }
        if (input.length() < 2) {
            return null;
        }
        Scored best = bestCorrection(input);
        if (best == null || best.distance > (input.length() <= 4 ? 1.0 : 2.0)
                || likelyUnfinished(best, completionRanks(input, 1))) {
            return null;
        }
        return words[best.rank];
    }

    /**
     * True when the input looks like the start of a common word rather than a typo: some
     * completion is no more than 20x rarer than the best correction ("hel" -> help, not he).
     */
    private static boolean likelyUnfinished(Scored correction, List<Integer> completionRanks) {
        return !completionRanks.isEmpty() && completionRanks.get(0) < (correction.rank + 10) * 20;
    }

    /** Frequency ranks of words that start with the prefix (excluding the prefix itself), most frequent first. */
    private List<Integer> completionRanks(String prefix, int limit) {
        int start = lowerBound(prefix);
        List<Integer> ranks = new ArrayList<>();
        for (int i = start; i < sortedKeys.length && sortedKeys[i].startsWith(prefix); i++) {
            if (!sortedKeys[i].equals(prefix)) {
                ranks.add(sortedRanks[i]);
            }
        }
        Collections.sort(ranks);
        return ranks.size() > limit ? new ArrayList<>(ranks.subList(0, limit)) : ranks;
    }

    private List<String> aliasCompletions(String input) {
        List<String> result = new ArrayList<>();
        for (Map.Entry<String, List<String>> entry : aliases.entrySet()) {
            if (!entry.getKey().equals(input) && entry.getKey().startsWith(input) && entry.getKey().length() <= input.length() + 2) {
                result.addAll(entry.getValue());
            }
        }
        return result;
    }

    private static final class Scored {
        final int rank;
        final double distance;
        final double score;

        Scored(int rank, double distance) {
            this.rank = rank;
            this.distance = distance;
            // Each edit costs about as much as being 20x rarer.
            this.score = distance * 3.0 + Math.log(rank + 10.0);
        }
    }

    private Scored bestCorrection(String input) {
        List<Scored> scored = scoreCorrections(input);
        return scored.isEmpty() ? null : scored.get(0);
    }

    private List<Scored> scoreCorrections(String input) {
        double maxDistance = input.length() <= 4 ? 1.0 : 2.0;
        List<Scored> scored = new ArrayList<>();
        for (int length = Math.max(1, input.length() - 2); length <= input.length() + 2; length++) {
            List<Integer> ranks = ranksByLength.get(length);
            if (ranks == null) continue;
            for (int rank : ranks) {
                String candidate = key(words[rank]);
                if (candidate.equals(input)) continue;
                double distance = distance(input, candidate, maxDistance);
                if (distance <= maxDistance) {
                    scored.add(new Scored(rank, distance));
                }
            }
        }
        scored.sort((a, b) -> Double.compare(a.score, b.score));
        return scored;
    }

    /** Damerau-Levenshtein distance where substituting a neighbouring key costs less. Stops early above {@code limit}. */
    static double distance(String a, String b, double limit) {
        int n = a.length(), m = b.length();
        if (Math.abs(n - m) > limit) return limit + 1;
        double[][] d = new double[n + 1][m + 1];
        for (int i = 0; i <= n; i++) d[i][0] = i;
        for (int j = 0; j <= m; j++) d[0][j] = j;
        for (int i = 1; i <= n; i++) {
            double rowMin = Double.MAX_VALUE;
            for (int j = 1; j <= m; j++) {
                char x = a.charAt(i - 1), y = b.charAt(j - 1);
                double substitution = x == y ? 0 : (adjacent(x, y) ? 0.6 : 1.0);
                double value = Math.min(Math.min(d[i - 1][j] + 1, d[i][j - 1] + 1), d[i - 1][j - 1] + substitution);
                if (i > 1 && j > 1 && x == b.charAt(j - 2) && a.charAt(i - 2) == y) {
                    value = Math.min(value, d[i - 2][j - 2] + 0.8);
                }
                d[i][j] = value;
                rowMin = Math.min(rowMin, value);
            }
            if (rowMin > limit) return limit + 1;
        }
        return d[n][m];
    }

    private static boolean adjacent(char a, char b) {
        int[] pa = keyPosition(a), pb = keyPosition(b);
        if (pa == null || pb == null) return false;
        return Math.abs(pa[0] - pb[0]) <= 1 && Math.abs(pa[1] - pb[1]) <= 1;
    }

    private static int[] keyPosition(char c) {
        String[] rows = QWERTY_ROWS.split("\\|");
        for (int row = 0; row < rows.length; row++) {
            int column = rows[row].indexOf(c);
            if (column >= 0) return new int[]{row, column};
        }
        return null;
    }

    private int lowerBound(String prefix) {
        int low = 0, high = sortedKeys.length;
        while (low < high) {
            int mid = (low + high) >>> 1;
            if (sortedKeys[mid].compareTo(prefix) < 0) low = mid + 1;
            else high = mid;
        }
        return low;
    }

    /** Lower-cases and keeps only letters, apostrophes and hyphens. */
    static String key(String text) {
        if (text == null || text.isEmpty()) return "";
        StringBuilder builder = new StringBuilder(text.length());
        for (int i = 0; i < text.length(); i++) {
            char ch = Character.toLowerCase(text.charAt(i));
            if ((ch >= 'a' && ch <= 'z') || ch == '\'' || ch == '-') builder.append(ch);
        }
        return builder.toString();
    }

    private static Map<String, List<String>> englishAliases() {
        LinkedHashMap<String, List<String>> aliases = new LinkedHashMap<>();
        String[][] pairs = {
                {"im", "I'm"}, {"ive", "I've"}, {"ill", "I'll"}, {"id", "I'd"},
                {"youre", "you're"}, {"youve", "you've"}, {"youll", "you'll"}, {"youd", "you'd"},
                {"hes", "he's"}, {"hed", "he'd"}, {"hell", "he'll"}, {"shes", "she's"}, {"shed", "she'd"}, {"shell", "she'll"},
                {"its", "it's", "its"}, {"itd", "it'd"}, {"itll", "it'll"},
                {"thats", "that's"}, {"theres", "there's"}, {"heres", "here's"}, {"whats", "what's"}, {"whos", "who's"},
                {"wheres", "where's"}, {"whens", "when's"}, {"whys", "why's"}, {"hows", "how's"},
                {"dont", "don't"}, {"doesnt", "doesn't"}, {"didnt", "didn't"}, {"cant", "can't"}, {"couldnt", "couldn't"},
                {"wouldnt", "wouldn't"}, {"shouldnt", "shouldn't"}, {"wont", "won't"}, {"isnt", "isn't"}, {"arent", "aren't"},
                {"wasnt", "wasn't"}, {"werent", "weren't"}, {"hasnt", "hasn't"}, {"havent", "haven't"}, {"hadnt", "hadn't"},
                {"mustnt", "mustn't"}, {"neednt", "needn't"}, {"mightnt", "mightn't"}, {"mustve", "must've"},
                {"couldve", "could've"}, {"wouldve", "would've"}, {"shouldve", "should've"},
                {"theyre", "they're"}, {"theyve", "they've"}, {"theyll", "they'll"}, {"theyd", "they'd"},
                {"weve", "we've"}, {"were", "we're", "were"}, {"well", "we'll", "well"}, {"wed", "we'd"},
                {"lets", "let's", "lets"}, {"thered", "there'd"}, {"therell", "there'll"},
                // Keep the core shortcuts available even if an OEM build omits optional assets.
                {"wdym", "what do you mean"}, {"idk", "I don't know"}, {"btw", "by the way"},
                {"omw", "on my way"}, {"asap", "as soon as possible"},
        };
        for (String[] pair : pairs) {
            aliases.put(pair[0], Arrays.asList(pair).subList(1, pair.length));
        }
        return aliases;
    }
}
