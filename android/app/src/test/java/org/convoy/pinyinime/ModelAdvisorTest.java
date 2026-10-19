package org.convoy.pinyinime;

import static org.junit.Assert.assertEquals;
import static org.junit.Assert.assertFalse;
import static org.junit.Assert.assertNotNull;
import static org.junit.Assert.assertNull;
import static org.junit.Assert.assertTrue;

import org.junit.Test;

public class ModelAdvisorTest {
    private static final long GB = ModelAdvisor.GB;

    /** A phone as Android reports it: "8 GB" phones report a little less than 8 GiB. */
    private static ModelAdvisor.DeviceSpecs phone(double totalGb, double freeGb, int cores) {
        ModelAdvisor.DeviceSpecs specs = new ModelAdvisor.DeviceSpecs();
        specs.totalRam = (long) (totalGb * GB);
        specs.availableRam = (long) (freeGb * GB);
        specs.cores = cores;
        specs.supported64Bit = true;
        specs.abi = "arm64-v8a";
        specs.freeStorage = 20 * GB;
        return specs;
    }

    @Test
    public void flagshipGetsTheLargerModel() {
        ModelAdvisor.Verdict verdict = ModelAdvisor.advise(phone(7.4, 3.0, 8));
        assertEquals("qwen3-1.7b", verdict.recommended.id);
        assertTrue(verdict.deviceSupported);
        assertEquals("half of the cores", 4, verdict.threads);
    }

    @Test
    public void sixGigabytePhoneStillFitsTheLargerModel() {
        assertEquals("qwen3-1.7b", ModelAdvisor.advise(phone(5.5, 2.0, 8)).recommended.id);
    }

    @Test
    public void midRangePhoneGetsTheSmallModel() {
        assertEquals("qwen3-0.6b", ModelAdvisor.advise(phone(3.6, 1.5, 8)).recommended.id);
    }

    @Test
    public void twoGigabytePhoneGetsNoModel() {
        ModelAdvisor.Verdict verdict = ModelAdvisor.advise(phone(1.8, 0.8, 4));
        assertNull(verdict.recommended);
        assertFalse(verdict.deviceSupported);
        assertTrue(verdict.reasons.get(0).contains("smallest model"));
    }

    @Test
    public void thirtyTwoBitOrFewCoresGetNoModel() {
        ModelAdvisor.DeviceSpecs old = phone(4, 2, 8);
        old.supported64Bit = false;
        old.abi = "armeabi-v7a";
        assertNull(ModelAdvisor.advise(old).recommended);
        assertNull(ModelAdvisor.advise(phone(8, 4, 2)).recommended);
        ModelAdvisor.DeviceSpecs lowRam = phone(8, 4, 8);
        lowRam.lowRamDevice = true;
        assertNull(ModelAdvisor.advise(lowRam).recommended);
    }

    @Test
    public void modelNeverTakesMoreThanAQuarterOfRam() {
        for (double total = 1; total <= 16; total += 0.5) {
            ModelAdvisor.Verdict verdict = ModelAdvisor.advise(phone(total, total / 2, 8));
            if (verdict.recommended != null) {
                assertTrue(verdict.recommended.runtimeBytes() <= total * GB * 0.25);
            }
        }
    }

    @Test
    public void loadIsRefusedWhenOtherAppsUseTheMemory() {
        ModelAdvisor.DeviceSpecs busy = phone(7.4, 1.5, 8);  // fine device, but little free right now
        assertNotNull(ModelAdvisor.loadProblem(busy, ModelAdvisor.CATALOG[0].fileBytes));
        assertNull(ModelAdvisor.loadProblem(phone(7.4, 3.5, 8), ModelAdvisor.CATALOG[0].fileBytes));
    }

    @Test
    public void hotOrBatterySavingDevicesAreThrottled() {
        ModelAdvisor.DeviceSpecs hot = phone(7.4, 4, 8);
        hot.thermalThrottled = true;
        assertNotNull(ModelAdvisor.loadProblem(hot, ModelAdvisor.CATALOG[1].fileBytes));
        ModelAdvisor.DeviceSpecs saver = phone(7.4, 4, 8);
        saver.powerSave = true;
        assertEquals(1, ModelAdvisor.threads(saver));
    }

    @Test
    public void importRejectsModelsTooLargeForTheDevice() {
        assertNotNull(ModelAdvisor.importProblem(phone(3.6, 2, 8), ModelAdvisor.CATALOG[0].fileBytes));
        assertNull(ModelAdvisor.importProblem(phone(3.6, 2, 8), ModelAdvisor.CATALOG[1].fileBytes));
        ModelAdvisor.DeviceSpecs full = phone(8, 4, 8);
        full.freeStorage = GB;
        assertNotNull(ModelAdvisor.importProblem(full, ModelAdvisor.CATALOG[0].fileBytes));
    }

    @Test
    public void recognisesCatalogModelsByChecksum() {
        assertEquals("qwen3-0.6b", ModelAdvisor.bySha256(ModelAdvisor.CATALOG[1].sha256.toUpperCase()).id);
        assertNull(ModelAdvisor.bySha256("00"));
    }

    @Test
    public void detectsTheLanguageToProofread() {
        assertEquals(GrammarLanguage.MANDARIN, GrammarLanguage.detect("我昨天去了商店。", false));
        assertEquals(GrammarLanguage.MANDARIN, GrammarLanguage.detect("我今天用 WiFi 上网", false));
        assertEquals(GrammarLanguage.MALAY, GrammarLanguage.detect("Saya tidak suka makan nasi", false));
        assertEquals(GrammarLanguage.ENGLISH, GrammarLanguage.detect("She go to school yesterday", true));
        assertEquals(GrammarLanguage.MALAY, GrammarLanguage.detect("Okay", true));
        assertEquals(GrammarLanguage.ENGLISH, GrammarLanguage.detect("Okay", false));
    }
}
