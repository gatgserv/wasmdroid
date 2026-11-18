package dev.wasmdroid.app

import androidx.test.ext.junit.runners.AndroidJUnit4
import dev.wasmdroid.app.engine.BrowserProfileLedger
import org.junit.Assert.*
import org.junit.Test
import org.junit.runner.RunWith

@RunWith(AndroidJUnit4::class)
class BrowserProfileLedgerTest {
    private class Store : BrowserProfileLedger.Persistence {
        val records = mutableMapOf<String, BrowserProfileLedger.Record>()
        var fail = false
        override fun read(profileId: String) = records[profileId]
        override fun write(profileId: String, record: BrowserProfileLedger.Record) {
            check(!fail) { "Fixture persistence failure" }; records[profileId] = record
        }
    }

    @Test fun defaultPreservesDataAndMultipleViewLeasesFenceOnlyTheirOwnProfile() {
        val store = Store(); val ledger = BrowserProfileLedger(store)
        assertEquals("preserved", ledger.snapshot("first").status); assertTrue(store.records.isEmpty())
        val first = ledger.acquire("first", "11111111-1111-1111-1111-111111111111"); val second = ledger.acquire("first", "11111111-1111-1111-1111-111111111111")
        assertEquals(first.profileName, second.profileName)
        assertNotEquals(first.profileName, ledger.acquire("other", "11111111-1111-1111-1111-111111111111").also { it.close() }.profileName)
        assertThrows(IllegalStateException::class.java) { ledger.beginErase("first", 0) }
        first.close(); first.close(); assertEquals(1, ledger.snapshot("first").activeViews)
        second.close(); assertEquals(0, ledger.snapshot("first").activeViews)
        assertTrue(store.records.isEmpty())
    }

    @Test fun durablePendingFenceSurvivesProcessInterruptionAndRequiresExplicitRetry() {
        val store = Store(); val firstProcess = BrowserProfileLedger(store)
        val old = firstProcess.beginErase("first", 0)
        assertEquals("clearing", firstProcess.snapshot("first").status)
        val restarted = BrowserProfileLedger(store)
        assertEquals("interrupted", restarted.snapshot("first").status)
        assertThrows(IllegalStateException::class.java) { restarted.acquire("first", "11111111-1111-1111-1111-111111111111") }
        assertThrows(IllegalStateException::class.java) { restarted.beginErase("first", 0) }
        val retry = restarted.beginErase("first", old.revision)
        assertFalse(firstProcess.completed(old)) // Exact persisted generation changed.
        assertFalse(firstProcess.failed(old))
        assertEquals("clearing", restarted.snapshot("first").status)
        assertTrue(restarted.completed(retry)); assertFalse(restarted.completed(retry))
        assertEquals("cleared", restarted.snapshot("first").status)
        restarted.acquire("first", "11111111-1111-1111-1111-111111111111").close()
    }

    @Test fun providerFailureStaysFencedAndAnotherProfileIsUntouched() {
        val store = Store(); val ledger = BrowserProfileLedger(store)
        val other = ledger.acquire("other", "11111111-1111-1111-1111-111111111111")
        val ticket = ledger.beginErase("first", 0)
        assertThrows(IllegalStateException::class.java) { ledger.beginErase("first", ticket.revision) }
        assertTrue(ledger.failed(ticket))
        assertThrows(IllegalStateException::class.java) { ledger.acquire("first", "11111111-1111-1111-1111-111111111111") }
        assertTrue(other.valid); assertNull(store.records["other"])
        val retry = ledger.beginErase("first", ledger.snapshot("first").revision)
        assertTrue(ledger.completed(retry)); other.close()
    }

    @Test fun receiptCommitFailureCannotAuthorizeDestructiveStartOrReopening() {
        val store = Store(); val ledger = BrowserProfileLedger(store)
        store.fail = true
        assertThrows(IllegalStateException::class.java) { ledger.beginErase("first", 0) }
        assertTrue(store.records.isEmpty()); ledger.acquire("first", "11111111-1111-1111-1111-111111111111").close()
        store.fail = false; val ticket = ledger.beginErase("first", 0)
        store.fail = true
        assertThrows(IllegalStateException::class.java) { ledger.completed(ticket) }
        assertThrows(IllegalStateException::class.java) { ledger.acquire("first", "11111111-1111-1111-1111-111111111111") }
        assertThrows(IllegalStateException::class.java) { ledger.failed(ticket) }
        store.fail = false
        val retry = ledger.beginErase("first", ledger.snapshot("first").revision)
        assertTrue(ledger.completed(retry))
    }

    @Test fun providersThatNeverAcknowledgeCannotCreateUnboundedDestructiveOperations() {
        val store = Store(); val ledger = BrowserProfileLedger(store)
        val pending = (0 until 16).map { ledger.beginErase("profile-$it", 0) }
        assertThrows(IllegalStateException::class.java) { ledger.beginErase("overflow", 0) }
        assertNull(store.records["overflow"])
        assertTrue(ledger.completed(pending.first()))
        val admitted = ledger.beginErase("overflow", 0)
        assertTrue(ledger.completed(admitted))
    }

    @Test fun replacementGenerationWaitsForEveryOldViewToBeDestroyed() {
        val ledger = BrowserProfileLedger(Store())
        val oldGeneration = "11111111-1111-1111-1111-111111111111"
        val newGeneration = "22222222-2222-2222-2222-222222222222"
        val first = ledger.acquire("browser:42", oldGeneration)
        val second = ledger.acquire("browser:42", oldGeneration)
        assertThrows(IllegalStateException::class.java) { ledger.acquire("browser:42", newGeneration) }
        assertThrows(IllegalStateException::class.java) { ledger.beginErase("browser:42", 0) }
        first.close()
        assertThrows(IllegalStateException::class.java) { ledger.acquire("browser:42", newGeneration) }
        second.close()
        val replacement = ledger.acquire("browser:42", newGeneration)
        assertEquals(first.profileName, replacement.profileName)
        replacement.close(); assertTrue(ledger.completed(ledger.beginErase("browser:42", 0)))
    }

    @Test fun privateNamespaceIsCanonicalAndLegacyProfileBytesKeepTheirExactName() {
        assertEquals("wasmdroid-7431caddeca04373d44df574de214daab6cd994b45ed659b359305eae1aa372e", BrowserProfileLedger.profileName("legacy-app"))
        assertNotEquals(BrowserProfileLedger.profileName("legacy-app"), BrowserProfileLedger.profileName("browser:1"))
        BrowserProfileLedger.profileName("browser:9223372036854775807")
        for (key in listOf("browser:0", "browser:01", "browser:-1", "browser:+1", "browser:9223372036854775808", "other:1"))
            assertThrows(IllegalArgumentException::class.java) { BrowserProfileLedger.profileName(key) }
    }
}
