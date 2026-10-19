package org.convoy.pinyinime;

import android.app.ActivityManager;
import android.content.Context;
import android.os.Build;
import android.os.PowerManager;

import java.io.BufferedReader;
import java.io.File;
import java.io.FileReader;
import java.util.Arrays;
import java.util.List;

/** Measures the hardware facts that {@link ModelAdvisor} needs. */
final class DeviceProfiler {
    /** ABIs the native grammar library is built for. */
    private static final List<String> NATIVE_ABIS = Arrays.asList("arm64-v8a", "x86_64");

    private DeviceProfiler() {
    }

    static ModelAdvisor.DeviceSpecs measure(Context context) {
        ModelAdvisor.DeviceSpecs specs = new ModelAdvisor.DeviceSpecs();
        ActivityManager activityManager = (ActivityManager) context.getSystemService(Context.ACTIVITY_SERVICE);
        ActivityManager.MemoryInfo memory = new ActivityManager.MemoryInfo();
        if (activityManager != null) {
            activityManager.getMemoryInfo(memory);
            specs.totalRam = memory.totalMem;
            // When the system is already low on memory, treat nothing as available.
            specs.availableRam = memory.lowMemory ? 0 : memory.availMem;
            specs.lowRamDevice = activityManager.isLowRamDevice();
        }
        specs.cores = Runtime.getRuntime().availableProcessors();
        specs.maxCpuKhz = maxCpuKhz();
        specs.abi = Build.SUPPORTED_ABIS.length > 0 ? Build.SUPPORTED_ABIS[0] : "unknown";
        specs.supported64Bit = false;
        for (String abi : Build.SUPPORTED_64_BIT_ABIS) {
            if (NATIVE_ABIS.contains(abi)) specs.supported64Bit = true;
        }
        specs.freeStorage = context.getFilesDir().getUsableSpace();
        PowerManager power = (PowerManager) context.getSystemService(Context.POWER_SERVICE);
        if (power != null) {
            specs.powerSave = power.isPowerSaveMode();
            if (Build.VERSION.SDK_INT >= 29) {
                specs.thermalThrottled = power.getCurrentThermalStatus() >= PowerManager.THERMAL_STATUS_SEVERE;
            }
        }
        return specs;
    }

    /** One line per fact, for the settings screen. */
    static String describe(ModelAdvisor.DeviceSpecs specs) {
        StringBuilder text = new StringBuilder();
        text.append("RAM: ").append(ModelAdvisor.size(specs.totalRam)).append(" total, ")
                .append(ModelAdvisor.size(specs.availableRam)).append(" free now\n");
        text.append("CPU: ").append(specs.cores).append(" cores");
        if (specs.maxCpuKhz >= 300_000) {  // some emulators report nonsense; skip implausible values
            text.append(String.format(java.util.Locale.ROOT, ", up to %.1f GHz", specs.maxCpuKhz / 1e6));
        }
        text.append(" (").append(specs.abi).append(")\n");
        text.append("Storage: ").append(ModelAdvisor.size(specs.freeStorage)).append(" free");
        if (specs.powerSave) text.append("\nBattery saver is on: the AI will use one CPU core.");
        return text.toString();
    }

    private static long maxCpuKhz() {
        long best = 0;
        File[] cpus = new File("/sys/devices/system/cpu").listFiles((dir, name) -> name.matches("cpu\\d+"));
        if (cpus == null) return 0;
        for (File cpu : cpus) {
            File file = new File(cpu, "cpufreq/cpuinfo_max_freq");
            try (BufferedReader reader = new BufferedReader(new FileReader(file))) {
                String line = reader.readLine();
                if (line != null) best = Math.max(best, Long.parseLong(line.trim()));
            } catch (Exception ignored) {
                // Not readable on every device; frequency is informational only.
            }
        }
        return best;
    }
}
