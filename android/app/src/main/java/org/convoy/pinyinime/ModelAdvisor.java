package org.convoy.pinyinime;

import java.util.ArrayList;
import java.util.List;
import java.util.Locale;

/**
 * Decides which grammar model, if any, a device can run while leaving room for other apps.
 *
 * <p>The model may use at most a quarter of total RAM and half of the RAM that is free when it
 * loads, it gets at most half of the CPU cores, and it needs a 64-bit CPU with enough cores to
 * answer in reasonable time.
 */
public final class ModelAdvisor {
    static final long MB = 1024L * 1024L;
    static final long GB = 1024L * MB;

    /** Fraction of total RAM the model may use, so the rest of the system and other apps keep working. */
    static final double TOTAL_RAM_SHARE = 0.25;
    /** Fraction of currently available RAM the model may take when it loads. */
    static final double AVAILABLE_RAM_SHARE = 0.5;
    static final int MIN_CORES = 4;
    static final long MIN_FREE_STORAGE_AFTER_IMPORT = 500 * MB;
    /** Prompt plus answer, in tokens. Bounds the KV cache. */
    static final int CONTEXT_TOKENS = 1024;

    public static final class Model {
        public final String id;
        public final String name;
        public final String fileName;
        public final String url;
        public final long fileBytes;
        public final String sha256;
        public final String note;

        Model(String id, String name, String fileName, String url, long fileBytes, String sha256, String note) {
            this.id = id;
            this.name = name;
            this.fileName = fileName;
            this.url = url;
            this.fileBytes = fileBytes;
            this.sha256 = sha256;
            this.note = note;
        }

        public long runtimeBytes() {
            return estimateRuntimeBytes(fileBytes);
        }
    }

    /** Larger models first; the advisor recommends the first one that fits. */
    public static final Model[] CATALOG = {
            new Model("qwen3-1.7b", "Qwen3 1.7B (Q4_K_M)", "Qwen3-1.7B-Q4_K_M.gguf",
                    "https://huggingface.co/unsloth/Qwen3-1.7B-GGUF/resolve/main/Qwen3-1.7B-Q4_K_M.gguf",
                    1_107_409_472L, "b139949c5bd74937ad8ed8c8cf3d9ffb1e99c866c823204dc42c0d91fa181897",
                    "Recommended. Fixes most everyday mistakes in English, Mandarin and Malay."),
            new Model("qwen3-0.6b", "Qwen3 0.6B (Q4_K_M)", "Qwen3-0.6B-Q4_K_M.gguf",
                    "https://huggingface.co/unsloth/Qwen3-0.6B-GGUF/resolve/main/Qwen3-0.6B-Q4_K_M.gguf",
                    396_705_472L, "ac2d97712095a558e31573f62f466a3f9d93990898b0ec79d7c974c1780d524a",
                    "Basic. Fast, but misses many mistakes and sometimes suggests wrong changes; review each suggestion."),
    };

    /** What the profiler measured. All sizes in bytes. */
    public static final class DeviceSpecs {
        public long totalRam;
        public long availableRam;
        public boolean lowRamDevice;
        public int cores;
        public long maxCpuKhz;
        public boolean supported64Bit;
        public String abi = "";
        public long freeStorage;
        public boolean thermalThrottled;
        public boolean powerSave;
    }

    public static final class Verdict {
        /** The model to download, or null when none is suitable. */
        public final Model recommended;
        public final boolean deviceSupported;
        public final long budgetBytes;
        public final int threads;
        public final List<String> reasons;

        Verdict(Model recommended, boolean deviceSupported, long budgetBytes, int threads, List<String> reasons) {
            this.recommended = recommended;
            this.deviceSupported = deviceSupported;
            this.budgetBytes = budgetBytes;
            this.threads = threads;
            this.reasons = reasons;
        }
    }

    private ModelAdvisor() {
    }

    /** Model weights are memory-mapped; add the KV cache (~120 MB at 1024 tokens) and compute buffers. */
    static long estimateRuntimeBytes(long fileBytes) {
        return fileBytes + 120 * MB + 180 * MB;
    }

    /** The most memory the model may use on this device in general (ignores what is free right now). */
    static long deviceBudget(DeviceSpecs specs) {
        return (long) (specs.totalRam * TOTAL_RAM_SHARE);
    }

    /** The most memory the model may use right now. */
    static long currentBudget(DeviceSpecs specs) {
        return Math.min(deviceBudget(specs), (long) (specs.availableRam * AVAILABLE_RAM_SHARE));
    }

    /** Half of the cores (at most 4) so foreground apps keep the rest; one thread in power-save mode. */
    static int threads(DeviceSpecs specs) {
        if (specs.powerSave) return 1;
        return Math.max(1, Math.min(4, specs.cores / 2));
    }

    public static Verdict advise(DeviceSpecs specs) {
        List<String> reasons = new ArrayList<>();
        long budget = deviceBudget(specs);
        boolean supported = true;
        if (!specs.supported64Bit) {
            reasons.add("This device has a 32-bit CPU (" + specs.abi + "); the on-device AI needs a 64-bit CPU.");
            supported = false;
        }
        if (specs.lowRamDevice) {
            reasons.add("Android reports this as a low-RAM device.");
            supported = false;
        }
        if (specs.cores < MIN_CORES) {
            reasons.add("Only " + specs.cores + " CPU cores; at least " + MIN_CORES + " are needed for usable speed.");
            supported = false;
        }
        Model choice = null;
        if (supported) {
            for (Model model : CATALOG) {
                if (model.runtimeBytes() <= budget) {
                    choice = model;
                    break;
                }
            }
            if (choice == null) {
                Model smallest = CATALOG[CATALOG.length - 1];
                reasons.add("The smallest model needs about " + size(smallest.runtimeBytes()) + " of RAM, but only "
                        + size(budget) + " (a quarter of " + size(specs.totalRam) + ") can be used without slowing other apps.");
            } else {
                reasons.add("Up to " + size(budget) + " of RAM (a quarter of " + size(specs.totalRam) + ") can be used for the AI; "
                        + choice.name + " needs about " + size(choice.runtimeBytes()) + ".");
                if (choice.fileBytes + MIN_FREE_STORAGE_AFTER_IMPORT > specs.freeStorage) {
                    reasons.add("Free storage is low: the model file needs " + size(choice.fileBytes) + " and "
                            + size(specs.freeStorage) + " is free.");
                }
            }
        }
        if (specs.thermalThrottled) {
            reasons.add("The device is hot right now; checks will be refused until it cools down.");
        }
        return new Verdict(choice, supported && choice != null, budget, threads(specs), reasons);
    }

    /** Null if a model file of this size may be imported, otherwise the reason it may not. */
    public static String importProblem(DeviceSpecs specs, long fileBytes) {
        if (!specs.supported64Bit || specs.lowRamDevice || specs.cores < MIN_CORES) {
            return advise(specs).reasons.get(0);
        }
        long needed = estimateRuntimeBytes(fileBytes);
        if (needed > deviceBudget(specs)) {
            return "This model needs about " + size(needed) + " of RAM, more than the " + size(deviceBudget(specs))
                    + " this device can spare. Choose a smaller model.";
        }
        if (fileBytes + MIN_FREE_STORAGE_AFTER_IMPORT > specs.freeStorage) {
            return "Not enough free storage: the file is " + size(fileBytes) + " and " + size(specs.freeStorage) + " is free.";
        }
        return null;
    }

    /** Null if a model of this size may load now, otherwise why not (checked before every load). */
    public static String loadProblem(DeviceSpecs specs, long fileBytes) {
        if (specs.thermalThrottled) {
            return "The device is too hot. Try again when it has cooled down.";
        }
        long needed = estimateRuntimeBytes(fileBytes);
        if (needed > currentBudget(specs)) {
            return "Not enough free memory right now (" + size(specs.availableRam) + " free, the AI needs "
                    + size(needed) + " and may use only half of what is free). Close some apps and try again.";
        }
        return null;
    }

    /** The catalog model with this checksum, or null for a model the user brought themselves. */
    public static Model bySha256(String sha256) {
        for (Model model : CATALOG) {
            if (model.sha256.equalsIgnoreCase(sha256)) return model;
        }
        return null;
    }

    static String size(long bytes) {
        if (bytes >= GB) return String.format(Locale.ROOT, "%.1f GB", bytes / (double) GB);
        return String.format(Locale.ROOT, "%d MB", bytes / MB);
    }
}
