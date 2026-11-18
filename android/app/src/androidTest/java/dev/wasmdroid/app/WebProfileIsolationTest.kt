package dev.wasmdroid.app

import android.webkit.WebView
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import androidx.webkit.WebViewCompat
import androidx.webkit.WebViewFeature
import dev.wasmdroid.app.engine.BrowserProfileLedger
import org.junit.Assert.*
import org.junit.Test
import org.junit.runner.RunWith
import java.util.UUID
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit

/** Browser profile evidence only. This does not certify guest execution,
 * stop/restart, WebView navigation policy or external browser isolation. */
@RunWith(AndroidJUnit4::class)
class WebProfileIsolationTest {
    @Test fun cookiesAreSeparateEvenAtIdenticalLoopbackHostAndPort() {
        val instrumentation=InstrumentationRegistry.getInstrumentation()
        val views=mutableListOf<WebView>()
        val prefix="profile-regression-${UUID.randomUUID()}"
        val address="http://127.0.0.1:49152/"
        try {
            instrumentation.runOnMainSync {
                assertTrue("Update the test device WebView: MULTI_PROFILE is required for embedded acceptance",WebViewFeature.isFeatureSupported(WebViewFeature.MULTI_PROFILE))
                repeat(2){index->
                    val view=WebView(instrumentation.targetContext)
                    WebViewCompat.setProfile(view,BrowserProfileLedger.profileName("$prefix-$index"))
                    views+=view
                }
                assertNotEquals(WebViewCompat.getProfile(views[0]).name,WebViewCompat.getProfile(views[1]).name)
            }
            val stored=CountDownLatch(1)
            instrumentation.runOnMainSync {
                WebViewCompat.getProfile(views[0]).cookieManager.setCookie(address,"wasmdroid_regression=first; Path=/"){success->
                    assertTrue(success);stored.countDown()
                }
            }
            assertTrue("Cookie write did not finish",stored.await(5,TimeUnit.SECONDS))
            instrumentation.runOnMainSync {
                val first=WebViewCompat.getProfile(views[0]).cookieManager
                val second=WebViewCompat.getProfile(views[1]).cookieManager
                assertTrue(first.getCookie(address).contains("wasmdroid_regression=first"))
                assertFalse(second.getCookie(address)?.contains("wasmdroid_regression=first")?:false)
                val reopened=WebView(instrumentation.targetContext)
                WebViewCompat.setProfile(reopened,BrowserProfileLedger.profileName("$prefix-0"))
                views+=reopened
                assertTrue(WebViewCompat.getProfile(reopened).cookieManager.getCookie(address).contains("wasmdroid_regression=first"))
            }
        } finally {
            instrumentation.runOnMainSync {
                views.forEach{view->
                    // Only uniquely named test profiles are touched. A loaded
                    // provider profile cannot be deleted until process restart.
                    WebViewCompat.getProfile(view).cookieManager.removeAllCookies(null)
                    view.destroy()
                }
            }
        }
    }
}
