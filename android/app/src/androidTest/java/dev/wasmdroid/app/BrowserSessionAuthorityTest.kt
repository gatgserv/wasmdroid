package dev.wasmdroid.app

import androidx.test.ext.junit.runners.AndroidJUnit4
import dev.wasmdroid.app.engine.BrowserSessionAuthority
import org.junit.Assert.*
import org.junit.Test
import org.junit.runner.RunWith
import java.util.UUID

/** Deterministic authority definitions; no actual port/WebView is exercised. */
@RunWith(AndroidJUnit4::class)
class BrowserSessionAuthorityTest {
    private fun ports(host: Int = 49152, guest: Int = 8080) = """[{"host_port":$host,"instance_port":$guest,"protocol":"tcp"}]"""
    private fun generation() = UUID.randomUUID().toString()

    @Test fun cachedPortAndWrongGenerationNeverGrantAuthority() {
        val authority = BrowserSessionAuthority(); val first = generation()
        assertThrows(IllegalStateException::class.java) { authority.acquire("first", first, 8080, "/") }
        authority.publish("first", first, ports(), "first")
        assertThrows(IllegalStateException::class.java) { authority.acquire("second", first, 8080, "/") }
        assertThrows(IllegalStateException::class.java) { authority.acquire("first", generation(), 8080, "/") }
        assertThrows(IllegalStateException::class.java) { authority.acquire("first", first, 80, "/") }
        val lease = authority.acquire("first", first, 8080, "/app?view=home")
        assertEquals("http://127.0.0.1:49152/app?view=home", lease.url)
        assertTrue(lease.permitsOrdinaryRequest(lease.url))
        for (url in listOf("http://127.0.0.1:49153/", "https://127.0.0.1:49152/", "http://localhost:49152/", "http://user@127.0.0.1:49152/", "file:///data/private", "http://example.invalid/")) assertFalse(lease.permitsOrdinaryRequest(url))
        lease.close()
    }

    @Test fun actualRevocationPrecedesCallbackAndStaleRevokeCannotTouchReusedPort() {
        val authority = BrowserSessionAuthority(); val first = generation(); val next = generation()
        authority.publish("first", first, ports(), "first")
        val old = authority.acquire("first", first, 8080, "/")
        var callbacks = 0
        old.onRevoked { assertFalse(old.valid); assertFalse(old.permitsOrdinaryRequest(old.url)); callbacks++; old.close() }
        authority.revoke("first", first)
        assertEquals(1, callbacks)
        authority.publish("second", next, ports(), "second")
        val current = authority.acquire("second", next, 8080, "/")
        authority.revoke("first", first)
        assertTrue(current.valid); assertFalse(old.valid)
        authority.close(); assertFalse(current.valid)
    }

    @Test fun activeMappingCannotBeSilentlyReassignedAndOnlyExactReplayIsIdempotent() {
        val authority = BrowserSessionAuthority(); val token = generation()
        authority.publish("first", token, ports(), "first"); authority.publish("first", token, ports(), "first")
        assertThrows(IllegalStateException::class.java) { authority.publish("first", generation(), ports(), "first") }
        assertThrows(IllegalStateException::class.java) { authority.publish("first", token, ports(49153), "first") }
        assertThrows(IllegalStateException::class.java) { authority.publish("second", generation(), ports(), "second") }
        authority.close()
        assertThrows(IllegalStateException::class.java) { authority.publish("first", token, ports(), "first") }
    }

    @Test fun strictNetworkGuaranteeIsUnsupportedAndPathsCannotEscapeLocalOrigin() {
        val authority = BrowserSessionAuthority(); val token = generation()
        authority.publish("first", token, ports(), "first")
        assertThrows(UnsupportedOperationException::class.java) { authority.acquire("first", token, 8080, "/", true) }
        for (path in listOf("https://example.invalid", "//example.invalid/", "/\\other", "/line\nbreak"))
            assertThrows(IllegalArgumentException::class.java) { authority.acquire("first", token, 8080, path) }
        val lease = authority.acquire("first", token, 8080, "/")
        assertFalse(lease.completeNetworkIsolation); assertFalse(lease.exclusiveTransportGateway)
        lease.close(); assertFalse(lease.valid)
    }

    @Test fun lateRevocationSubscriberAndSessionAdmissionLimitAreBounded() {
        val authority = BrowserSessionAuthority(); val token = generation()
        authority.publish("first", token, ports(), "first")
        val leases = (0 until 64).map { authority.acquire("first", token, 8080, "/") }
        assertThrows(IllegalStateException::class.java) { authority.acquire("first", token, 8080, "/") }
        leases.first().close(); val replacement = authority.acquire("first", token, 8080, "/")
        authority.revoke("first", token)
        var called = false; replacement.onRevoked { called = true }; assertTrue(called)
        leases.forEach { it.close() }; replacement.close()
    }

    @Test fun replacementKeepsOnlyTheNativeOwnedProfileIdentity() {
        val authority = BrowserSessionAuthority(); val first = generation(); val next = generation()
        authority.publish("old-actual-id", first, ports(), "browser:42")
        val old = authority.acquire("old-actual-id", first, 8080, "/")
        assertEquals("browser:42", old.profileId)
        assertThrows(IllegalStateException::class.java) { authority.publish("old-actual-id", first, ports(), "browser:43") }
        authority.revoke("old-actual-id", first)
        authority.publish("new-actual-id", next, ports(), "browser:42")
        val replacement = authority.acquire("new-actual-id", next, 8080, "/")
        assertEquals(old.profileId, replacement.profileId); assertNotEquals(old.instanceId, replacement.instanceId)
        assertFalse(old.valid); assertTrue(replacement.valid)
        authority.close()
    }
}
