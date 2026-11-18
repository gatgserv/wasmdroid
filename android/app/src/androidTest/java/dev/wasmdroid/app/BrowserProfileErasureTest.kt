package dev.wasmdroid.app

import android.webkit.WebView
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import androidx.webkit.ProfileStore
import androidx.webkit.WebViewFeature
import androidx.webkit.WebViewCompat
import dev.wasmdroid.app.engine.BrowserProfileLease
import dev.wasmdroid.app.engine.BrowserProfiles
import dev.wasmdroid.app.engine.BrowserSessionAuthority
import dev.wasmdroid.app.engine.BrowserSessionLease
import org.json.JSONObject
import org.junit.Assert.*
import org.junit.Test
import org.junit.runner.RunWith
import java.util.UUID
import java.util.concurrent.CountDownLatch
import java.util.concurrent.ExecutionException
import java.util.concurrent.TimeUnit

/** Actual-provider definition only: unique test-owned profiles, no service,
 * network server or production browser data. Not yet executed evidence. */
@RunWith(AndroidJUnit4::class)
class BrowserProfileErasureTest {
    @Test fun sameApplicationReplacementPreservesCookiesAndOldViewBlocksReopeningAndClear() {
        val instrumentation = InstrumentationRegistry.getInstrumentation()
        val profiles = BrowserProfiles.get(instrumentation.targetContext)
        val original = "browser-retry-${UUID.randomUUID()}"
        val replacement = "browser-replacement-${UUID.randomUUID()}"
        val firstGeneration = UUID.randomUUID().toString(); val nextGeneration = UUID.randomUUID().toString()
        val authority = BrowserSessionAuthority()
        val ports = """[{"host_port":49154,"instance_port":8080,"protocol":"tcp"}]"""
        val views = mutableListOf<WebView>(); val leases = mutableListOf<BrowserProfileLease>()
        val address = "http://127.0.0.1:49154/"
        try {
            authority.publish(original, firstGeneration, ports, original)
            val first = authority.acquire(original, firstGeneration, 8080, "/")
            val stored = CountDownLatch(1)
            instrumentation.runOnMainSync {
                val lease = profiles.acquire(first); leases += lease
                val view = WebView(instrumentation.targetContext); profiles.attach(view, lease); views += view
                WebViewCompat.getProfile(view).cookieManager.setCookie(address, "wasmdroid_retry=preserved; Path=/") { success -> assertTrue(success); stored.countDown() }
            }
            assertTrue(stored.await(5, TimeUnit.SECONDS))
            authority.revoke(original, firstGeneration)
            authority.publish(replacement, nextGeneration, ports, original)
            val next = authority.acquire(replacement, nextGeneration, 8080, "/")
            instrumentation.runOnMainSync {
                assertThrows(IllegalStateException::class.java) { profiles.acquire(next) }
            }
            assertThrows(ExecutionException::class.java) { profiles.erase(original, 0).get(5, TimeUnit.SECONDS) }
            instrumentation.runOnMainSync {
                views[0].destroy(); leases[0].close()
                val lease = profiles.acquire(next); leases += lease
                val view = WebView(instrumentation.targetContext); profiles.attach(view, lease); views += view
                assertEquals(leases[0].profileName, lease.profileName)
                assertTrue(WebViewCompat.getProfile(view).cookieManager.getCookie(address)?.contains("wasmdroid_retry=preserved") ?: false)
            }
            first.close(); next.close()
        } finally {
            instrumentation.runOnMainSync {
                views.forEachIndexed { index, view -> runCatching { view.destroy() }; leases.getOrNull(index)?.close() }
                authority.close()
            }
            if (leases.isNotEmpty()) profiles.erase(original, JSONObject(profiles.state(original)).getLong("revision")).get(15, TimeUnit.SECONDS)
        }
    }

    @Test fun explicitClearAcknowledgesOnlySelectedProfileAndLivingViewsFenceErase() {
        val instrumentation = InstrumentationRegistry.getInstrumentation()
        val profiles = BrowserProfiles.get(instrumentation.targetContext)
        val ids = List(2) { "browser-erase-${UUID.randomUUID()}" }
        val views = mutableListOf<WebView>()
        val leases = mutableListOf<BrowserProfileLease>()
        val authority = BrowserSessionAuthority()
        val sessions = mutableListOf<BrowserSessionLease>()
        val address = "http://127.0.0.1:49152/"
        try {
            instrumentation.runOnMainSync {
                assertTrue("Provider MULTI_PROFILE is required", WebViewFeature.isFeatureSupported(WebViewFeature.MULTI_PROFILE))
                assertTrue("Provider DELETE_BROWSING_DATA is required", WebViewFeature.isFeatureSupported(WebViewFeature.DELETE_BROWSING_DATA))
                ids.forEachIndexed { index, id ->
                    val generation = UUID.randomUUID().toString()
                    val ports = """[{"host_port":${49152+index},"instance_port":8080,"protocol":"tcp"}]"""
                    authority.publish(id, generation, ports, id)
                    val session = authority.acquire(id, generation, 8080, "/"); sessions += session
                    val lease = profiles.acquire(session); leases += lease
                    val view = WebView(instrumentation.targetContext); profiles.attach(view, lease); views += view
                    val workerSettings = WebViewCompat.getProfile(view).serviceWorkerController.serviceWorkerWebSettings
                    assertFalse(workerSettings.allowFileAccess); assertFalse(workerSettings.allowContentAccess)
                }
            }
            val stored = CountDownLatch(2)
            instrumentation.runOnMainSync {
                leases.forEachIndexed { index, lease ->
                    ProfileStore.getInstance().getOrCreateProfile(lease.profileName).cookieManager
                        .setCookie(address, "wasmdroid_clear_test=profile$index; Path=/") { success -> assertTrue(success); stored.countDown() }
                }
            }
            assertTrue("Profile cookie writes did not complete", stored.await(5, TimeUnit.SECONDS))
            assertThrows(ExecutionException::class.java) { profiles.erase(ids[0], 0).get(5, TimeUnit.SECONDS) }
            assertEquals(1, JSONObject(profiles.state(ids[0])).getInt("active_views"))
            instrumentation.runOnMainSync { views[0].destroy(); leases[0].close() }
            val receipt = JSONObject(profiles.erase(ids[0], 0).get(15, TimeUnit.SECONDS))
            assertEquals("cleared", receipt.getString("status")); assertFalse(receipt.getBoolean("pending_erasure"))
            instrumentation.runOnMainSync {
                val first = ProfileStore.getInstance().getOrCreateProfile(leases[0].profileName).cookieManager.getCookie(address)
                val second = ProfileStore.getInstance().getOrCreateProfile(leases[1].profileName).cookieManager.getCookie(address)
                assertFalse(first?.contains("wasmdroid_clear_test") ?: false)
                assertTrue(second?.contains("wasmdroid_clear_test=profile1") ?: false)
                assertTrue(leases[1].valid)
            }
        } finally {
            instrumentation.runOnMainSync {
                views.forEachIndexed { index, view -> runCatching { view.destroy() }; leases.getOrNull(index)?.close() }
                sessions.forEach { it.close() }; authority.close()
            }
            // Only these unique fixture profiles are cleared. Never use global
            // cookies/WebStorage cleanup, including when an assertion failed.
            ids.filter { id -> leases.any { it.profileId == id } }.forEach { id ->
                val revision = JSONObject(profiles.state(id)).getLong("revision")
                profiles.erase(id, revision).get(15, TimeUnit.SECONDS)
            }
        }
    }
}
