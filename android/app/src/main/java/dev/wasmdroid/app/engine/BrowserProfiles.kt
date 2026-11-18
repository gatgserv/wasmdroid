package dev.wasmdroid.app.engine

import android.annotation.SuppressLint
import android.content.Context
import android.os.Handler
import android.os.Looper
import android.webkit.WebView
import androidx.webkit.ProfileStore
import androidx.webkit.WebStorageCompat
import androidx.webkit.WebViewCompat
import androidx.webkit.WebViewFeature
import org.json.JSONObject
import java.util.concurrent.CompletableFuture

/** App-process singleton: active WebViews and service-side erasure share leases.
 * Default profile/global CookieManager/WebStorage deletion is never used. */
class BrowserProfiles private constructor(context: Context) {
    private val main = Handler(Looper.getMainLooper())
    private val preferences = context.getSharedPreferences("browser-profile-receipts", Context.MODE_PRIVATE)
    private val ledger = BrowserProfileLedger(object : BrowserProfileLedger.Persistence {
        override fun read(profileId: String): BrowserProfileLedger.Record? {
            val raw = preferences.getString(BrowserProfileLedger.profileName(profileId), null) ?: return null
            check(raw.length <= 2048) { "Browser profile receipt exceeds limit" }
            val json = JSONObject(raw)
            check(json.getInt("schema_version") == 1) { "Unsupported browser profile receipt" }
            return BrowserProfileLedger.Record(json.getLong("revision"), json.optString("erase_generation").takeIf { it.isNotEmpty() }, json.getString("outcome"))
        }
        // The KTX edit helper discards the commit result this receipt requires.
        @SuppressLint("UseKtx")
        override fun write(profileId: String, record: BrowserProfileLedger.Record) {
            val json = JSONObject().put("schema_version", 1).put("revision", record.revision).put("outcome", record.outcome)
            record.eraseGeneration?.let { json.put("erase_generation", it) }
            check(preferences.edit().putString(BrowserProfileLedger.profileName(profileId), json.toString()).commit()) { "Browser profile receipt could not be persisted" }
        }
    })

    fun capabilities(): String = JSONObject().put("isolated_profiles", supported(WebViewFeature.MULTI_PROFILE))
        .put("profile_data_erasure", supported(WebViewFeature.MULTI_PROFILE) && supported(WebViewFeature.DELETE_BROWSING_DATA))
        .put("complete_browser_egress_isolation", false).put("exclusive_generation_gateway", false)
        .put("profile_directory_wipe", false)
        .put("retention_default", "preserve").toString()

    fun state(profileId: String): String {
        val state = ledger.snapshot(profileId)
        return JSONObject().put("revision", state.revision).put("active_views", state.activeViews)
            .put("status", state.status).put("pending_erasure", state.eraseGeneration != null)
            .put("erasure_scope", "website_data_stored_before_request")
            .put("retention", "preserve").put("capabilities", JSONObject(capabilities())).toString()
    }

    /** Main only: acquire before WebView construction; setProfile is still the
     * first WebView API after construction. Close only AFTER destroy returns. */
    fun acquire(session: BrowserSessionLease): BrowserProfileLease {
        checkMain()
        check(session.valid) { "Browser session ended before profile admission" }
        if (!supported(WebViewFeature.MULTI_PROFILE)) throw UnsupportedOperationException("The installed WebView provider does not support private application profiles")
        return ledger.acquire(session.profileId, session.generation)
    }

    fun attach(view: WebView, lease: BrowserProfileLease) {
        checkMain(); check(lease.valid) { "Browser profile lease ended" }
        if (WebViewFeature.isFeatureSupported(WebViewFeature.MULTI_PROFILE)) {
            WebViewCompat.setProfile(view, lease.profileName)
            val profile = WebViewCompat.getProfile(view)
            // The profile's service-worker settings are independent of WebSettings.
            profile.serviceWorkerController.serviceWorkerWebSettings.apply {
                allowFileAccess = false
                allowContentAccess = false
            }
            profile.cookieManager.setAcceptThirdPartyCookies(view, false)
        } else {
            throw UnsupportedOperationException("The installed WebView provider does not support private application profiles")
        }
    }

    /** Explicit whole-profile website-data clear, not a ProfileStore directory
     * wipe. No timeout fabricates completion; owner retains the returned future
     * until the actual provider callback. Missing provider capability is rejected
     * before the destructive operation/pending receipt begins. */
    fun erase(profileId: String, expectedRevision: Long): CompletableFuture<String> {
        val answer = CompletableFuture<String>()
        val posted = main.post {
            var ticket: BrowserProfileLedger.EraseTicket? = null
            try {
                if (WebViewFeature.isFeatureSupported(WebViewFeature.MULTI_PROFILE)) {
                    if (WebViewFeature.isFeatureSupported(WebViewFeature.DELETE_BROWSING_DATA)) {
                        val captured = ledger.beginErase(profileId, expectedRevision)
                        ticket = captured
                        // Loading provider state is reserved for explicit erasure.
                        val profile = ProfileStore.getInstance().getOrCreateProfile(BrowserProfileLedger.profileName(profileId))
                        WebStorageCompat.deleteBrowsingData(profile.webStorage) {
                            try {
                                check(ledger.completed(captured)) { "Browser erasure generation changed" }
                                answer.complete(state(profileId))
                            } catch (failure: Exception) {
                                // Persisted pending state keeps future profile acquisition
                                // fenced if receipt commit failed after provider clearing.
                                runCatching { ledger.failed(captured) }
                                answer.completeExceptionally(failure)
                            }
                        }
                        return@post
                    }
                }
                throw UnsupportedOperationException("The installed WebView provider cannot clear all website data for one application profile")
            } catch (failure: Exception) {
                ticket?.let { runCatching { ledger.failed(it) } }
                answer.completeExceptionally(failure)
            }
        }
        if (!posted) answer.completeExceptionally(IllegalStateException("Browser main thread is unavailable"))
        return answer
    }

    private fun supported(feature: String): Boolean = WebViewFeature.isFeatureSupported(feature)
    private fun checkMain() { check(Looper.myLooper() == Looper.getMainLooper()) { "Browser provider operation requires the main thread" } }
    companion object {
        @Volatile private var instance: BrowserProfiles? = null
        fun get(context: Context): BrowserProfiles = instance ?: synchronized(this) {
            instance ?: BrowserProfiles(context.applicationContext).also { instance = it }
        }
    }
}
