package dev.wasmdroid.app

import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import androidx.webkit.WebViewFeature
import dev.wasmdroid.app.engine.BrowserProfiles
import org.json.JSONObject
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertThrows
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith
import java.util.UUID
import java.util.concurrent.ExecutionException
import java.util.concurrent.TimeUnit

/** Tests the provider's capability contract without clearing a profile. */
@RunWith(AndroidJUnit4::class)
class BrowserProfileCapabilityTest {
    @Test
    fun profilesAndErasureAreAdvertisedSeparatelyAndUnsupportedEraseDoesNotIssueReceipt() {
        val context = InstrumentationRegistry.getInstrumentation().targetContext
        val profiles = BrowserProfiles.get(context)
        val canCreateProfiles = WebViewFeature.isFeatureSupported(WebViewFeature.MULTI_PROFILE)
        val canErase = canCreateProfiles && WebViewFeature.isFeatureSupported(WebViewFeature.DELETE_BROWSING_DATA)
        val id = "browser-unsupported-erase-${UUID.randomUUID()}"
        val initial = JSONObject(profiles.state(id))
        val capabilities = initial.getJSONObject("capabilities")
        assertEquals(canCreateProfiles, capabilities.getBoolean("isolated_profiles"))
        assertEquals(canErase, capabilities.getBoolean("profile_data_erasure"))
        assertEquals("preserved", initial.getString("status"))
        assertFalse(initial.getBoolean("pending_erasure"))
        assertEquals(0L, initial.getLong("revision"))
        assertEquals(0, initial.getInt("active_views"))

        if (!canErase) {
            val rejected = assertThrows(ExecutionException::class.java) {
                profiles.erase(id, initial.getLong("revision")).get(5, TimeUnit.SECONDS)
            }
            assertTrue(rejected.cause is UnsupportedOperationException)

            val retained = JSONObject(profiles.state(id))
            assertEquals("preserved", retained.getString("status"))
            assertFalse(retained.getBoolean("pending_erasure"))
            assertEquals(0L, retained.getLong("revision"))
            assertEquals(0, retained.getInt("active_views"))
        }
    }
}
