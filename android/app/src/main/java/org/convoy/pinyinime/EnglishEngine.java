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
import java.util.LinkedHashMap;
import java.util.LinkedHashSet;
import java.util.List;
import java.util.Map;

public class EnglishEngine {
    private static final String WORDS_ASSET = "english_words.txt";
    private static final String SHORTCUTS_ASSET = "english_shortcuts.tsv";
    private static final int MAX_CANDIDATES = 32;

    private static final List<String> WORDS = new ArrayList<>();
    private static final Map<String, List<String>> ALIASES = buildAliases();
    private static volatile boolean loaded;

    public EnglishEngine(Context context) {
        ensureLoaded(context.getApplicationContext().getAssets());
    }

    public List<String> getCandidates(String rawInput) {
        String input = normalize(rawInput);
        if (input.isEmpty()) {
            return Collections.emptyList();
        }

        LinkedHashSet<String> matches = new LinkedHashSet<>();
        addAliasMatches(matches, input);
        for (String word : WORDS) {
            if (!word.startsWith(input)) {
                continue;
            }
            matches.add(word);
            if (matches.size() >= MAX_CANDIDATES) {
                break;
            }
        }

        if (matches.isEmpty()) {
            return Collections.singletonList(input);
        }

        ArrayList<String> results = new ArrayList<>(Math.min(MAX_CANDIDATES, matches.size() + 1));
        String first = null;
        for (String match : matches) {
            first = match;
            results.add(match);
            break;
        }
        if (!input.equals(first)) {
            results.add(input);
        }
        for (String match : matches) {
            if (match.equals(first) || match.equals(input)) {
                continue;
            }
            results.add(match);
            if (results.size() >= MAX_CANDIDATES) {
                break;
            }
        }
        return results;
    }

    private static void addAliasMatches(LinkedHashSet<String> results, String input) {
        List<String> exact = ALIASES.get(input);
        if (exact != null) {
            results.addAll(exact);
        }
        for (Map.Entry<String, List<String>> entry : ALIASES.entrySet()) {
            if (results.size() >= MAX_CANDIDATES) {
                return;
            }
            if (entry.getKey().equals(input) || !entry.getKey().startsWith(input)) {
                continue;
            }
            results.addAll(entry.getValue());
        }
    }

    private static Map<String, List<String>> buildAliases() {
        LinkedHashMap<String, List<String>> aliases = new LinkedHashMap<>();
        alias(aliases, "im", "I'm");
        alias(aliases, "ive", "I've");
        alias(aliases, "ill", "I'll");
        alias(aliases, "id", "I'd");
        alias(aliases, "youre", "you're");
        alias(aliases, "youve", "you've");
        alias(aliases, "youll", "you'll");
        alias(aliases, "youd", "you'd");
        alias(aliases, "hes", "he's");
        alias(aliases, "hed", "he'd");
        alias(aliases, "hell", "he'll");
        alias(aliases, "shes", "she's");
        alias(aliases, "shed", "she'd");
        alias(aliases, "shell", "she'll");
        alias(aliases, "its", "it's", "its");
        alias(aliases, "itd", "it'd");
        alias(aliases, "itll", "it'll");
        alias(aliases, "thats", "that's");
        alias(aliases, "theres", "there's");
        alias(aliases, "heres", "here's");
        alias(aliases, "whats", "what's");
        alias(aliases, "whos", "who's");
        alias(aliases, "wheres", "where's");
        alias(aliases, "whens", "when's");
        alias(aliases, "whys", "why's");
        alias(aliases, "hows", "how's");
        alias(aliases, "dont", "don't");
        alias(aliases, "doesnt", "doesn't");
        alias(aliases, "didnt", "didn't");
        alias(aliases, "cant", "can't");
        alias(aliases, "couldnt", "couldn't");
        alias(aliases, "wouldnt", "wouldn't");
        alias(aliases, "shouldnt", "shouldn't");
        alias(aliases, "wont", "won't");
        alias(aliases, "isnt", "isn't");
        alias(aliases, "arent", "aren't");
        alias(aliases, "wasnt", "wasn't");
        alias(aliases, "werent", "weren't");
        alias(aliases, "hasnt", "hasn't");
        alias(aliases, "havent", "haven't");
        alias(aliases, "hadnt", "hadn't");
        alias(aliases, "mustnt", "mustn't");
        alias(aliases, "neednt", "needn't");
        alias(aliases, "mightnt", "mightn't");
        alias(aliases, "mustve", "must've");
        alias(aliases, "couldve", "could've");
        alias(aliases, "wouldve", "would've");
        alias(aliases, "shouldve", "should've");
        alias(aliases, "theyre", "they're");
        alias(aliases, "theyve", "they've");
        alias(aliases, "theyll", "they'll");
        alias(aliases, "theyd", "they'd");
        alias(aliases, "weve", "we've");
        alias(aliases, "were", "we're", "were");
        alias(aliases, "well", "we'll", "well");
        alias(aliases, "wed", "we'd");
        alias(aliases, "lets", "let's", "lets");
        alias(aliases, "thered", "there'd");
        alias(aliases, "therell", "there'll");
        // Keep the core shortcuts available even if an OEM build omits optional assets.
        alias(aliases, "wdym", "what do you mean");
        alias(aliases, "idk", "I don't know");
        alias(aliases, "btw", "by the way");
        alias(aliases, "omw", "on my way");
        alias(aliases, "asap", "as soon as possible");
        return aliases;
    }

    private static void alias(Map<String, List<String>> aliases, String key, String... outputs) {
        List<String> values = new ArrayList<>(outputs.length);
        Collections.addAll(values, outputs);
        aliases.put(key, values);
    }

    private static void ensureLoaded(AssetManager assets) {
        if (loaded) {
            return;
        }
        synchronized (EnglishEngine.class) {
            if (loaded) {
                return;
            }
            WORDS.clear();
            loadShortcuts(assets);
            try (InputStream stream = assets.open(WORDS_ASSET);
                 BufferedReader reader = new BufferedReader(new InputStreamReader(stream, StandardCharsets.UTF_8))) {
                String line;
                while ((line = reader.readLine()) != null) {
                    String word = normalize(line);
                    if (!word.isEmpty()) {
                        WORDS.add(word);
                    }
                }
            } catch (IOException e) {
                throw new IllegalStateException("Failed to load english word asset", e);
            }
            loaded = true;
        }
    }

    private static void loadShortcuts(AssetManager assets) {
        try (InputStream stream = assets.open(SHORTCUTS_ASSET);
             BufferedReader reader = new BufferedReader(new InputStreamReader(stream, StandardCharsets.UTF_8))) {
            String line;
            while ((line = reader.readLine()) != null) {
                String[] parts = line.split("\\t", -1);
                if (parts.length < 2 || parts[0].trim().isEmpty() || parts[1].trim().isEmpty()) {
                    continue;
                }
                alias(ALIASES, parts[0].trim(), parts[1].trim());
            }
        } catch (IOException e) {
            throw new IllegalStateException("Failed to load English shortcut asset", e);
        }
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
            }
        }
        return builder.toString();
    }
}
