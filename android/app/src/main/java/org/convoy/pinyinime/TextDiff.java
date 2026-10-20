package org.convoy.pinyinime;

import java.util.ArrayList;
import java.util.List;

/**
 * Word-level diff for Latin text and character-level diff for Chinese, used to show what the
 * grammar check changed.
 */
final class TextDiff {
    enum Kind { SAME, REMOVED, ADDED }

    static final class Part {
        final Kind kind;
        final String text;

        Part(Kind kind, String text) {
            this.kind = kind;
            this.text = text;
        }

        @Override
        public String toString() {
            return (kind == Kind.SAME ? "" : kind == Kind.ADDED ? "+" : "-") + "[" + text + "]";
        }
    }

    private TextDiff() {
    }

    static List<Part> diff(String before, String after) {
        List<String> a = tokens(before), b = tokens(after);
        int n = a.size(), m = b.size();
        int[][] lcs = new int[n + 1][m + 1];
        for (int i = n - 1; i >= 0; i--) {
            for (int j = m - 1; j >= 0; j--) {
                lcs[i][j] = a.get(i).equals(b.get(j)) ? lcs[i + 1][j + 1] + 1 : Math.max(lcs[i + 1][j], lcs[i][j + 1]);
            }
        }
        List<Part> parts = new ArrayList<>();
        int i = 0, j = 0;
        while (i < n || j < m) {
            if (i < n && j < m && a.get(i).equals(b.get(j))) {
                append(parts, Kind.SAME, a.get(i++));
                j++;
            } else if (i < n && (j == m || lcs[i + 1][j] >= lcs[i][j + 1])) {
                append(parts, Kind.REMOVED, a.get(i++));  // removals first: "-go +went"
            } else {
                append(parts, Kind.ADDED, b.get(j++));
            }
        }
        return parts;
    }

    static boolean changed(String before, String after) {
        return !before.trim().equals(after.trim());
    }

    /** Words with their trailing spaces kept separate; every CJK character is its own token. */
    static List<String> tokens(String text) {
        List<String> tokens = new ArrayList<>();
        StringBuilder word = new StringBuilder();
        for (int i = 0; i < text.length(); i++) {
            char c = text.charAt(i);
            boolean cjk = Character.UnicodeScript.of(c) == Character.UnicodeScript.HAN
                    || (c >= '　' && c <= 'ヿ') || (c >= '＀' && c <= '￯');
            if (Character.isLetterOrDigit(c) && !cjk || c == '\'' || c == '-') {
                word.append(c);
                continue;
            }
            if (word.length() > 0) {
                tokens.add(word.toString());
                word.setLength(0);
            }
            tokens.add(String.valueOf(c));
        }
        if (word.length() > 0) tokens.add(word.toString());
        return tokens;
    }

    private static void append(List<Part> parts, Kind kind, String text) {
        if (!parts.isEmpty() && parts.get(parts.size() - 1).kind == kind) {
            Part last = parts.remove(parts.size() - 1);
            parts.add(new Part(kind, last.text + text));
        } else {
            parts.add(new Part(kind, text));
        }
    }
}
