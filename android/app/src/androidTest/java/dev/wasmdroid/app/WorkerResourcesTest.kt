package dev.wasmdroid.app

import androidx.test.ext.junit.runners.AndroidJUnit4
import dev.wasmdroid.app.ui.WorkerSample
import org.junit.Assert.*
import org.junit.Test
import org.junit.runner.RunWith

@RunWith(AndroidJUnit4::class)
class WorkerResourcesTest {
    @Test fun cpuIntervalsNeverSpanWorkerGenerationsOrInventMissingMeasurements() {
        val before=WorkerSample("one",49152,100_000_000,1_000_000_000)
        val after=before.copy(cpuNs=600_000_000,monotonicNs=3_000_000_000)
        assertEquals(25.0,after.cpuPercent(before)!!,0.0001)
        assertNull(after.cpuPercent(null))
        assertNull(after.copy(generation="two").cpuPercent(before))
        assertNull(after.copy(cpuNs=null).cpuPercent(before))
        assertNull(after.copy(cpuNs=1).cpuPercent(before))
        assertNull(after.copy(monotonicNs=before.monotonicNs).cpuPercent(before))
        val busy=after.copy(cpuNs=4_100_000_000)
        assertEquals(200.0,busy.cpuPercent(before)!!,0.0001)
        assertEquals(0.0,after.copy(cpuNs=before.cpuNs).cpuPercent(before)!!,0.0001)
    }
    @Test fun unavailableFieldsRemainNullAndScopeIsChecked() {
        val sample=WorkerSample.parse("""{"generation":"one","scope":"worker_process","memory_bytes":null,"cpu_time_ns":null,"sampled_at_monotonic_ns":null}""")
        assertNull(sample.residentBytes);assertNull(sample.cpuNs);assertNull(sample.monotonicNs)
        try{WorkerSample.parse("""{"generation":"one","scope":"guest_memory"}""");fail("Unsupported scope must not be displayed as process memory")}
        catch(expected:IllegalArgumentException){assertTrue(expected.message!!.contains("scope"))}
    }

    @Test fun measuringStartsOnlyWhenTheCurrentWorkerHasABaseline() {
        val sample = WorkerSample("one", 49152, 100_000_000, 1_000_000_000)
        assertTrue(sample.startsCpuIntervalAfter(null))
        assertTrue(sample.startsCpuIntervalAfter(sample.copy(generation = "old")))
        assertTrue(sample.startsCpuIntervalAfter(sample.copy(cpuNs = null)))
        assertFalse(sample.copy(cpuNs = null).startsCpuIntervalAfter(null))
        assertFalse(sample.copy(monotonicNs = null).startsCpuIntervalAfter(null))
        assertFalse(sample.startsCpuIntervalAfter(sample))
    }
}
