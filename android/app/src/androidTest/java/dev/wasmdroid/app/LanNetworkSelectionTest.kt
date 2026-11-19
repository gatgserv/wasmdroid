package dev.wasmdroid.app

import android.os.ParcelFileDescriptor
import android.system.Os
import android.system.OsConstants
import dev.wasmdroid.app.engine.LanSelectionLedger
import dev.wasmdroid.app.engine.LanSocketBinding
import dev.wasmdroid.app.engine.LanWifiCandidate
import org.junit.Assert.*
import org.junit.Test
import java.io.FileDescriptor
import java.io.IOException
import java.util.concurrent.Executors
import java.util.concurrent.TimeUnit

/** Pure callback-order/capability cases do not execute Android network binding.
 * The two descriptor cases separately require actual Android FD APIs; neither
 * attests a Wi-Fi interface, permission grant, incoming connection or QR flow. */
class LanNetworkSelectionTest {
    private fun ready(ledger:LanSelectionLedger, handle:Long=7, ip:String="192.168.1.8") {
        ledger.available(handle)
        ledger.capabilities(handle, wifi=true, notVpn=true, suspended=false, cellular=false)
        ledger.links(handle, "wlan-fixture", listOf(ip))
        ledger.blocked(handle, false)
    }
    private fun refuses(action:()->Unit) {
        try { action(); fail("Stale or unobserved LAN authority must be rejected") }
        catch (_:IllegalArgumentException) { }
        catch (_:IllegalStateException) { }
    }

    @Test fun modernCandidateWaitsForOrderedAvailableCapabilitiesLinksAndUnblocked() {
        val ledger = LanSelectionLedger(true)
        ledger.capabilities(7, true, true, false, false); ledger.links(7, "wlan-fixture", listOf("192.168.1.8"))
        ledger.blocked(7, false); assertTrue(ledger.candidates().isEmpty())
        ledger.available(7); assertTrue(ledger.candidates().isEmpty())
        ledger.links(7, "wlan-fixture", listOf("192.168.1.8")); assertTrue(ledger.candidates().isEmpty())
        ledger.capabilities(7, true, true, false, false); assertTrue(ledger.candidates().isEmpty())
        ledger.blocked(7, false)
        val candidate = ledger.candidates().single()
        assertTrue(candidate.blockedStatusObserved)
        assertEquals(7L, candidate.networkHandle)
        assertEquals("192.168.1.8", candidate.privateAddress)
    }

    @Test fun api28KeepsBlockedStatusExplicitlyUnobservedAndNeverInventsAGrant() {
        val ledger = LanSelectionLedger(false)
        ledger.available(7); ledger.capabilities(7, true, true, false, false)
        ledger.links(7, "wlan-fixture", listOf("192.168.1.8"))
        val candidate = ledger.candidates().single()
        assertFalse(candidate.blockedStatusObserved)
        ledger.blocked(7, true) // Not an available callback on this platform contract.
        assertSame(candidate, ledger.candidates().single())
        refuses { ledger.capture(candidate) { false } }
        assertTrue(ledger.capture(candidate) { true }.valid)
    }

    @Test fun cellularVpnSuspensionAndMissingInterfaceCannotOfferACandidate() {
        for (bits in 0..7) {
            val ledger = LanSelectionLedger(true)
            ledger.available(7)
            ledger.capabilities(7, (bits and 1) != 0, (bits and 2) != 0, (bits and 4) != 0, false)
            ledger.links(7, "wlan-fixture", listOf("192.168.1.8")); ledger.blocked(7, false)
            assertEquals(bits == 3, ledger.candidates().isNotEmpty())
        }
        val ledger = LanSelectionLedger(true); ready(ledger)
        ledger.links(7, null, listOf("192.168.1.8")); assertTrue(ledger.candidates().isEmpty())
    }

    @Test fun onlyCanonicalRfc1918LiteralsAreAcceptedWithoutDnsOrFallback() {
        for (ip in listOf("10.1.2.3","172.16.1.2","172.31.255.254","192.168.1.8"))
            assertEquals(ip, LanSelectionLedger.privateIpv4(ip))
        for (ip in listOf("0.0.0.0","127.0.0.1","169.254.1.8","100.64.1.1","8.8.8.8",
            "172.15.1.1","172.32.1.1","224.0.0.1","255.255.255.255","::1","private.invalid",
            "192.168.001.8","192.168.1.8/24","192.168.1.8\n","10.999.1.2"))
            assertNull(LanSelectionLedger.privateIpv4(ip))
    }

    @Test fun optionalIpv6UsesNumericUnicastAndChangesRevokeTheWholeWifiCapture() {
        val ledger = LanSelectionLedger(true)
        ready(ledger)
        val address = LanSelectionLedger.wifiIpv6("2001:db8:8::42", 64)!!
        assertTrue(LanSelectionLedger.sameIpv6Address(address.address, "2001:db8:8::42"))
        for (value in listOf("::", "::1", "::ffff:192.168.1.8", "fe80::1", "ff02::1", "wifi.local"))
            assertNull(LanSelectionLedger.wifiIpv6(value, 64))
        assertNull(LanSelectionLedger.wifiIpv6("2001:db8:8::42", 0))
        ledger.links(7, "wlan-fixture", listOf("192.168.1.8"), address)
        val candidate = ledger.candidates().single()
        assertEquals(address.address, candidate.ipv6Address)
        assertEquals(64, candidate.ipv6PrefixLength)
        val capture = ledger.capture(candidate) { true }
        ledger.links(7, "wlan-fixture", listOf("192.168.1.8"))
        assertFalse(capture.valid)
        assertNull(ledger.candidates().single().ipv6Address)
    }

    @Test fun assignedOptimisticIpv6CanListenButTentativeOnlyAndDuplicateAddressesCannot() {
        val tentative = OsConstants.IFA_F_TENTATIVE
        val optimistic = OsConstants.IFA_F_OPTIMISTIC
        val duplicate = OsConstants.IFA_F_DADFAILED
        assertTrue(LanSelectionLedger.ipv6UsableForListening(0))
        assertTrue(LanSelectionLedger.ipv6UsableForListening(OsConstants.IFA_F_DEPRECATED))
        assertTrue(LanSelectionLedger.ipv6UsableForListening(tentative or optimistic))
        assertFalse(LanSelectionLedger.ipv6UsableForListening(tentative))
        assertFalse(LanSelectionLedger.ipv6UsableForListening(tentative or optimistic or duplicate))
    }

    @Test fun mixedWifiCellularCapabilitiesRevokeAndNeedAFreshWifiOnlyCapture() {
        val ledger = LanSelectionLedger(true); ready(ledger)
        val original = ledger.candidates().single()
        val capture = ledger.capture(original) { true }; var invalidated = 0
        capture.onInvalidated { invalidated++ }
        ledger.capabilities(7, wifi=true, notVpn=true, suspended=false, cellular=true)
        assertTrue(ledger.candidates().isEmpty()); assertFalse(capture.valid)
        assertEquals(1, invalidated)
        refuses { ledger.capture(original) { true } }
        ledger.capabilities(7, wifi=true, notVpn=true, suspended=false, cellular=false)
        val replacement = ledger.candidates().single()
        assertTrue(replacement.interfaceEpoch > original.interfaceEpoch)
        assertNotSame(original, replacement); assertFalse(capture.valid)
        assertTrue(ledger.capture(replacement) { true }.valid)
    }

    @Test fun equivalentObservationsKeepTheExactCandidateAndCaptureStable() {
        val ledger = LanSelectionLedger(true); ready(ledger)
        ledger.links(7, "wlan-fixture", listOf("192.168.1.9","192.168.1.8"))
        val candidate = ledger.candidates().first()
        val capture = ledger.capture(candidate) { true }
        ledger.capabilities(7, true, true, false, false); ledger.blocked(7, false)
        ledger.links(7, "wlan-fixture", listOf("192.168.1.8","192.168.1.9","192.168.1.8"))
        assertSame(candidate, ledger.candidates().first()); assertTrue(capture.valid)
    }

    @Test fun addressOrInterfaceChangesInvalidateAndNeverResurrectAnOldCapture() {
        val ledger = LanSelectionLedger(true); ready(ledger)
        val first = ledger.candidates().single()
        val capture = ledger.capture(first) { true }; var callbacks = 0
        capture.onInvalidated { callbacks++ }
        ledger.links(7, "wlan-replacement", listOf("192.168.1.8"))
        assertFalse(capture.valid); assertEquals(1, callbacks)
        assertTrue(ledger.candidates().single().interfaceEpoch > first.interfaceEpoch)
        ledger.links(7, "wlan-fixture", listOf("192.168.1.8"))
        assertFalse(capture.valid); refuses { ledger.capture(first) { true } }
        val second = ledger.capture(ledger.candidates().single()) { true }
        ledger.links(7, "wlan-fixture", listOf("192.168.1.9")); assertFalse(second.valid)
    }

    @Test fun blockingLossUnavailableAndCloseFenceBeforeAnyNewUse() {
        val ledger = LanSelectionLedger(true); ready(ledger)
        val original = ledger.capture(ledger.candidates().single()) { true }
        ledger.blocked(7, true); assertFalse(original.valid); assertTrue(ledger.candidates().isEmpty())
        ledger.blocked(7, false)
        val active = ledger.capture(ledger.candidates().single()) { true }
        ledger.lost(7); assertFalse(active.valid); assertTrue(ledger.candidates().isEmpty())
        ledger.links(7, "wlan-fixture", listOf("192.168.1.8")); assertTrue(ledger.candidates().isEmpty())
        ready(ledger); val beforeUnavailable = ledger.capture(ledger.candidates().single()) { true }
        ledger.unavailable(); assertFalse(beforeUnavailable.valid); assertTrue(ledger.candidates().isEmpty())
        ready(ledger); val beforeClose = ledger.capture(ledger.candidates().single()) { true }
        ledger.close(); assertFalse(beforeClose.valid); assertTrue(ledger.candidates().isEmpty())
        ready(ledger); assertTrue(ledger.candidates().isEmpty())
    }

    @Test fun copiedForeignAndReusedNetworkHandleValuesAreNotCapabilities() {
        val ledger = LanSelectionLedger(true); ready(ledger)
        val original = ledger.candidates().single()
        val copied = LanWifiCandidate(original.issuer, original.networkHandle, original.interfaceEpoch,
            original.privateAddress, original.blockedStatusObserved)
        refuses { ledger.capture(copied) { true } }
        val other = LanSelectionLedger(true); ready(other)
        refuses { other.capture(original) { true } }
        val capture = ledger.capture(original) { true }
        assertFalse(other.isCurrent(capture) { true })
        ready(ledger) // Same native handle returned by a fresh onAvailable lifecycle.
        assertFalse(capture.valid); refuses { ledger.capture(original) { true } }
        assertFalse(original.toString().contains(original.privateAddress))
    }

    @Test fun explicitAdmissionLossInvalidatesAndOnRequiresANewCapture() {
        val ledger = LanSelectionLedger(true); ready(ledger); var allowed = true
        val candidate = ledger.candidates().single(); val capture = ledger.capture(candidate) { allowed }
        allowed = false; assertFalse(capture.valid)
        allowed = true; assertFalse(capture.valid)
        val next = ledger.capture(candidate) { allowed }; assertTrue(next.valid); assertNotSame(capture, next)
        next.close(); assertFalse(next.valid)
    }

    @Test fun lossDuringBindingRunsOutsideLedgerLockAndRejectsTheCompletedBind() {
        val ledger = LanSelectionLedger(true); ready(ledger)
        val capture = ledger.capture(ledger.candidates().single()) { true }
        val worker = Executors.newSingleThreadExecutor()
        try {
            refuses {
                ledger.withCurrent(capture, { true }) {
                    // A real concurrent callback must run before the platform
                    // call returns, without being blocked by a held ledger lock.
                    worker.submit { ledger.lost(7) }.get(2, TimeUnit.SECONDS)
                }
            }
            assertFalse(capture.valid)
        } finally { worker.shutdownNow() }
    }

    @Test fun oneThrowingObserverCannotPreventOtherInvalidationsAndLimitsStayBounded() {
        val ledger = LanSelectionLedger(true); ready(ledger)
        val first = ledger.capture(ledger.candidates().single()) { true }
        val second = ledger.capture(ledger.candidates().single()) { true }; var notified = 0
        first.onInvalidated { error("private observer error") }; second.onInvalidated { notified++ }
        ledger.lost(7); assertFalse(first.valid); assertFalse(second.valid); assertEquals(1, notified)
        second.close(); assertEquals(1, notified)
        refuses { second.onInvalidated {} }
        for (handle in 1L..8L) ledger.available(handle)
        refuses { ledger.available(9) }; refuses { ledger.available(0) }
    }

    @Test fun actualAndroidDuplicateIsClosedWhileNativeOwnerDescriptorRemainsOpen() {
        val pair = ParcelFileDescriptor.createSocketPair()
        try {
            var borrowed:FileDescriptor? = null
            LanSocketBinding.bindDuplicate(pair[0].fd) { duplicate -> borrowed = duplicate; Os.fstat(duplicate) }
            assertNotNull(borrowed); assertFalse(requireNotNull(borrowed).valid())
            assertTrue(pair[0].fileDescriptor.valid()); Os.fstat(pair[0].fileDescriptor)
        } finally { pair.forEach { it.close() } }
    }

    @Test fun actualAndroidBindFailureStillClosesOnlyTheDuplicate() {
        val pair = ParcelFileDescriptor.createSocketPair()
        try {
            var borrowed:FileDescriptor? = null
            try {
                LanSocketBinding.bindDuplicate(pair[0].fd) { duplicate ->
                    borrowed = duplicate; throw IOException("fixture binding refused")
                }
                fail("Expected injected platform refusal")
            } catch (_:IOException) { }
            assertNotNull(borrowed); assertFalse(requireNotNull(borrowed).valid())
            assertTrue(pair[0].fileDescriptor.valid()); Os.fstat(pair[0].fileDescriptor)
        } finally { pair.forEach { it.close() } }
    }
}
