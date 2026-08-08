package org.convoy.pinyinime;

import android.content.Context;
import android.content.res.AssetManager;

import java.io.BufferedReader;
import java.io.IOException;
import java.io.InputStream;
import java.io.InputStreamReader;
import java.nio.charset.StandardCharsets;
import java.util.ArrayList;
import java.util.Collections;
import java.util.LinkedHashSet;
import java.util.List;
import java.util.NavigableMap;
import java.util.TreeMap;

public class PinyinEngine {
    public enum ScriptMode {
        SIMPLIFIED,
        TRADITIONAL
    }

    private static final String LEXICON_ASSET = "pinyin_lexicon.tsv";
    private static final String SHORTCUTS_ASSET = "pinyin_shortcuts.tsv";
    private static final int MAX_CANDIDATES = 64;

    private static final class Entry {
        final List<String> simplified;
        final List<String> traditional;

        Entry(List<String> simplified, List<String> traditional) {
            this.simplified = simplified;
            this.traditional = traditional;
        }
    }

    private static final NavigableMap<String, Entry> LEXICON = new TreeMap<>();
    private static final NavigableMap<String, Entry> SHORTCUTS = new TreeMap<>();
    private static volatile boolean loaded;

    public PinyinEngine(Context context) {
        ensureLoaded(context.getApplicationContext());
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
        Entry shortcut = SHORTCUTS.get(input);
        if (shortcut != null) {
            addAll(results, shortcut, scriptMode);
        }
        Entry exact = LEXICON.get(input);
        if (exact != null) {
            addAll(results, exact, scriptMode);
        }

        for (var iter = LEXICON.tailMap(input, false).entrySet().iterator(); iter.hasNext() && results.size() < MAX_CANDIDATES; ) {
            var mapEntry = iter.next();
            if (!mapEntry.getKey().startsWith(input)) {
                break;
            }
            addAll(results, mapEntry.getValue(), scriptMode);
        }

        return new ArrayList<>(results);
    }

    private static void addAll(LinkedHashSet<String> results, Entry entry, ScriptMode scriptMode) {
        List<String> source = scriptMode == ScriptMode.TRADITIONAL ? entry.traditional : entry.simplified;
        for (String candidate : source) {
            if (!candidate.isEmpty()) {
                results.add(candidate);
            }
            if (results.size() >= MAX_CANDIDATES) {
                return;
            }
        }
    }

    private static void ensureLoaded(Context context) {
        if (loaded) {
            return;
        }
        synchronized (PinyinEngine.class) {
            if (loaded) {
                return;
            }
            loadFromAssets(context.getAssets());
            loaded = true;
        }
    }

    private static void loadFromAssets(AssetManager assets) {
        LEXICON.clear();
        loadShortcuts(assets);
        try (InputStream stream = assets.open(LEXICON_ASSET);
             BufferedReader reader = new BufferedReader(new InputStreamReader(stream, StandardCharsets.UTF_8))) {
            String line;
            while ((line = reader.readLine()) != null) {
                String[] parts = line.split("\t", -1);
                if (parts.length < 3) {
                    continue;
                }
                String key = normalize(parts[0]);
                if (key.isEmpty()) {
                    continue;
                }
                List<String> simplified = splitCandidates(parts[1]);
                List<String> traditional = splitCandidates(parts[2]);
                LEXICON.put(key, new Entry(simplified, traditional));
            }
        } catch (IOException e) {
            throw new IllegalStateException("Failed to load pinyin lexicon asset", e);
        }
    }

    private static void loadShortcuts(AssetManager assets) {
        try (InputStream stream = assets.open(SHORTCUTS_ASSET);
             BufferedReader reader = new BufferedReader(new InputStreamReader(stream, StandardCharsets.UTF_8))) {
            String line;
            while ((line = reader.readLine()) != null) {
                String[] parts = line.split("\\t", -1);
                if (parts.length < 3) {
                    continue;
                }
                String key = normalize(parts[0]);
                if (!key.isEmpty()) {
                    SHORTCUTS.put(key, new Entry(splitCandidates(parts[1]), splitCandidates(parts[2])));
                }
            }
        } catch (IOException e) {
            throw new IllegalStateException("Failed to load pinyin shortcut asset", e);
        }
    }

    private static List<String> splitCandidates(String block) {
        if (block == null || block.isEmpty()) {
            return Collections.emptyList();
        }
        String[] parts = block.split("\\|");
        List<String> result = new ArrayList<>(parts.length);
        for (String part : parts) {
            if (!part.isEmpty()) {
                result.add(part);
            }
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
            if (ch >= 'a' && ch <= 'z') {
                builder.append(ch);
            } else if (ch == '\'' || ch == 'v') {
                builder.append(ch);
            } else if (ch == 'ü') {
                builder.append('v');
            }
        }
        return builder.toString();
    }
}
