package org.convoy.pinyinime;

import android.content.res.AssetManager;

import java.io.BufferedReader;
import java.io.IOException;
import java.io.InputStream;
import java.io.InputStreamReader;
import java.nio.charset.StandardCharsets;
import java.util.ArrayList;
import java.util.Collections;
import java.util.LinkedHashMap;
import java.util.LinkedHashSet;
import java.util.List;
import java.util.Map;

final class ExtraLanguageEngine {
    private static final int MAX_CANDIDATES = 32;
    private final Map<String, List<String>> japanese = new LinkedHashMap<>();
    private final Map<String, List<String>> korean = new LinkedHashMap<>();

    ExtraLanguageEngine(AssetManager assets) {
        load(assets, "japanese_shortcuts.tsv", japanese);
        load(assets, "korean_shortcuts.tsv", korean);
    }

    List<String> getCandidates(String rawInput, boolean japaneseMode) {
        String input = normalize(rawInput);
        if (input.isEmpty()) return Collections.emptyList();
        Map<String, List<String>> source = japaneseMode ? japanese : korean;
        LinkedHashSet<String> results = new LinkedHashSet<>();
        List<String> exact = source.get(input);
        if (exact != null) results.addAll(exact);
        for (Map.Entry<String, List<String>> entry : source.entrySet()) {
            if (entry.getKey().startsWith(input)) results.addAll(entry.getValue());
            if (results.size() >= MAX_CANDIDATES) break;
        }
        if (results.isEmpty()) results.add(rawInput);
        return new ArrayList<>(results).subList(0, Math.min(MAX_CANDIDATES, results.size()));
    }

    private static void load(AssetManager assets, String name, Map<String, List<String>> target) {
        try (InputStream stream = assets.open(name); BufferedReader reader = new BufferedReader(new InputStreamReader(stream, StandardCharsets.UTF_8))) {
            String line;
            while ((line = reader.readLine()) != null) {
                String[] parts = line.split("\\t", -1);
                if (parts.length < 2) continue;
                String key = normalize(parts[0]);
                if (key.isEmpty()) continue;
                target.computeIfAbsent(key, ignored -> new ArrayList<>()).add(parts[1]);
            }
        } catch (IOException error) {
            throw new IllegalStateException("Failed to load " + name, error);
        }
    }

    private static String normalize(String value) {
        if (value == null) return "";
        StringBuilder result = new StringBuilder();
        for (int index = 0; index < value.length(); index++) {
            char character = Character.toLowerCase(value.charAt(index));
            if (character >= 'a' && character <= 'z') result.append(character);
        }
        return result.toString();
    }
}
