package org.convoy.pinyinime;

import java.io.BufferedReader;
import java.io.IOException;
import java.io.InputStream;
import java.io.InputStreamReader;
import java.nio.charset.StandardCharsets;

/** Opens bundled assets; lets the engines run on the JVM in unit tests as well as on Android. */
public interface AssetOpener {
    InputStream open(String name) throws IOException;

    interface LineHandler {
        void line(String line);
    }

    /** Streams an asset line by line. */
    static void forEachLine(AssetOpener assets, String name, LineHandler handler) {
        try (InputStream stream = assets.open(name);
             BufferedReader reader = new BufferedReader(new InputStreamReader(stream, StandardCharsets.UTF_8))) {
            String line;
            while ((line = reader.readLine()) != null) {
                handler.line(line);
            }
        } catch (IOException e) {
            throw new IllegalStateException("Failed to load asset " + name, e);
        }
    }

    /** Like {@link #forEachLine} but treats a missing asset as empty. */
    static void forEachLineIfPresent(AssetOpener assets, String name, LineHandler handler) {
        try {
            assets.open(name).close();
        } catch (IOException missing) {
            return;
        }
        forEachLine(assets, name, handler);
    }
}
