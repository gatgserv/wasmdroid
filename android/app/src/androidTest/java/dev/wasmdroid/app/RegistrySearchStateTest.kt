package dev.wasmdroid.app

import dev.wasmdroid.app.ui.*
import kotlinx.coroutines.*
import org.junit.Assert.*
import org.junit.Test

/** Pure source-state definitions. Deferred replies intentionally outlive cancelled
 * observation. No registry request, Android service or Internet probe is executed. */
class RegistrySearchStateTest {
    private data class Call(val query:String,val cursor:String?,val answer:CompletableDeferred<RegistrySearchPage> = CompletableDeferred())
    private class Backend:RegistrySearchBackend {
        override var current=true
        val calls=mutableListOf<Call>()
        override suspend fun search(query:String,cursor:String?):RegistrySearchPage {
            val call=Call(query,cursor);calls+=call
            return withContext(NonCancellable){call.answer.await()}
        }
    }
    private class Fixture:AutoCloseable {
        val scope=CoroutineScope(SupervisorJob()+Dispatchers.Unconfined)
        val backends=mutableListOf(Backend())
        var backend:Backend=backends.first()
        val failures=mutableListOf<Exception>()
        val controller=RegistrySearchController(scope,{backend},{failures+=it})
        init{controller.availability(true)}
        fun replacement(){backend.current=false;controller.availability(false);backend=Backend().also{backends+=it};controller.availability(true)}
        override fun close(){controller.close();backends.flatMap{it.calls}.forEach{it.answer.complete(page())};scope.cancel()}
    }
    private fun fixture(block:(Fixture)->Unit){Fixture().use(block)}
    private companion object {
        fun row(ref:String,text:String=ref)=RegistrySearchRow(ref,"{\"reference\":\"$ref\",\"description\":\"$text\"}")
        fun page(vararg refs:String,cursor:String?=null)=RegistrySearchPage(refs.map{row(it)},cursor)
    }

    @Test fun queryEditingDoesNotRetagSubmittedOrDisplayedResults()=fixture{f->
        f.controller.search("first query")
        assertEquals(RegistrySearchPhase.LoadingFirst,f.controller.state.value.phase)
        f.controller.edit("different draft")
        f.backend.calls[0].answer.complete(page("one",cursor="after1"))
        val state=f.controller.state.value
        assertEquals("different draft",state.draftQuery);assertEquals("first query",state.resultsQuery)
        assertEquals("first query",state.requestedQuery);assertTrue(state.showingPreviousResults)
        f.controller.next();assertEquals("first query",f.backend.calls[1].query);assertEquals("after1",f.backend.calls[1].cursor)
    }
    @Test fun newerQueryEpochRejectsLateSuccessAndFailureWithoutReporting()=fixture{f->
        f.controller.search("old");val old=f.backend.calls.last();val firstEpoch=f.controller.state.value.requestEpoch
        f.controller.search("new");assertTrue(f.controller.state.value.requestEpoch>firstEpoch)
        f.backend.calls.last().answer.complete(page("new-row"));old.answer.complete(page("old-row"))
        assertEquals(listOf("new-row"),f.controller.state.value.rows.map{it.reference});assertTrue(f.failures.isEmpty())
        f.controller.search("stale failure");val stale=f.backend.calls.last()
        f.controller.search("winner");f.backend.calls.last().answer.complete(page("winner-row"))
        stale.answer.completeExceptionally(IllegalStateException("private arbitrary error"))
        assertEquals("winner",f.controller.state.value.resultsQuery);assertTrue(f.failures.isEmpty())
    }
    @Test fun nextPageRetainsCursorAndAppendsUniqueReferencesWithoutReplacingEarlierRows()=fixture{f->
        f.controller.search("q");f.backend.calls.last().answer.complete(page("one","two",cursor="a"))
        f.controller.next();f.controller.next()
        assertEquals(2,f.backend.calls.size);assertEquals(RegistrySearchPhase.LoadingNext,f.controller.state.value.phase)
        f.backend.calls.last().answer.complete(RegistrySearchPage(listOf(row("two","changed"),row("three"),row("three")),null))
        assertEquals(listOf("one","two","three"),f.controller.state.value.rows.map{it.reference})
        assertEquals(row("two"),f.controller.state.value.rows[1]);assertEquals(2,f.controller.state.value.pages)
        assertNull(f.controller.state.value.nextCursor);assertFalse(f.controller.state.value.canNext)
    }
    @Test fun choosingSortPreservesLoadedRowsAndPaginationWithoutARequest() = fixture { f ->
        f.controller.search("packages")
        f.backend.calls.single().answer.complete(page("one", "two", cursor = "after"))
        val original = f.controller.state.value

        f.controller.setOrder(RegistrySearchOrder.NewestRelease)
        assertEquals(original.copy(order = RegistrySearchOrder.NewestRelease), f.controller.state.value)
        assertEquals(1, f.backend.calls.size)

        f.controller.next()
        assertEquals("after", f.backend.calls.last().cursor)
        f.backend.calls.last().answer.complete(page("three"))
        assertEquals(RegistrySearchOrder.NewestRelease, f.controller.state.value.order)
        assertEquals(listOf("one", "two", "three"), f.controller.state.value.rows.map { it.reference })
    }
    @Test fun firstFailureKeepsPreviousResultsAndRetryUsesFailedQueryNotDraft()=fixture{f->
        f.controller.search("old");f.backend.calls.last().answer.complete(page("retained"))
        f.controller.search("new");f.backend.calls.last().answer.completeExceptionally(Exception("never retained"))
        f.controller.edit("unsent")
        assertEquals("old",f.controller.state.value.resultsQuery);assertTrue(f.controller.state.value.showingPreviousResults)
        assertEquals(RegistrySearchProblem.Failed,f.controller.state.value.problem);assertEquals(1,f.failures.size)
        f.controller.retry();assertEquals("new",f.backend.calls.last().query);assertNull(f.backend.calls.last().cursor)
        f.backend.calls.last().answer.complete(page());assertTrue(f.controller.state.value.empty)
        assertEquals("new",f.controller.state.value.resultsQuery);assertEquals("unsent",f.controller.state.value.draftQuery)
    }
    @Test fun failedNextPageRetriesExactCursorAndDoesNotDiscardAcceptedPage()=fixture{f->
        f.controller.search("q");f.backend.calls.last().answer.complete(page("one",cursor="a"))
        f.controller.next();f.backend.calls.last().answer.completeExceptionally(Exception("failure"))
        f.controller.edit("different");f.controller.retry()
        assertEquals("q",f.backend.calls.last().query);assertEquals("a",f.backend.calls.last().cursor)
        f.backend.calls.last().answer.complete(page("two"))
        assertEquals(listOf("one","two"),f.controller.state.value.rows.map{it.reference})
    }
    @Test fun cancelledObserverCannotPublishOrReportAndRetryIsExplicit()=fixture{f->
        f.controller.search("q");val abandoned=f.backend.calls.last();f.controller.cancel()
        abandoned.answer.complete(page("abandoned"))
        assertEquals(RegistrySearchPhase.Cancelled,f.controller.state.value.phase);assertTrue(f.controller.state.value.rows.isEmpty())
        assertEquals(1,f.backend.calls.size);assertTrue(f.failures.isEmpty())
        f.controller.retry();f.backend.calls.last().answer.complete(page("retry"))
        assertEquals(listOf("retry"),f.controller.state.value.rows.map{it.reference})
    }
    @Test fun ownerReconnectDoesNotAutoFetchAndOldOwnerCannotReplaceNewOwner()=fixture{f->
        f.controller.search("q");val old=f.backend.calls.last()
        f.replacement();assertEquals(RegistrySearchPhase.Unavailable,f.controller.state.value.phase)
        assertTrue(f.backend.calls.isEmpty());old.answer.complete(page("old-owner"))
        assertTrue(f.controller.state.value.rows.isEmpty());assertTrue(f.failures.isEmpty())
        f.controller.retry();f.backend.calls.last().answer.complete(page("new-owner"))
        assertEquals(listOf("new-owner"),f.controller.state.value.rows.map{it.reference})
    }
    @Test fun acceptedResultsDisconnectThenReconnectOffersExactFirstPageRetryWithoutAutoFetch()=fixture{f->
        f.controller.search("accepted");f.backend.calls.last().answer.complete(page("one",cursor="old-cursor"))
        assertFalse(f.controller.state.value.canRetry)
        f.controller.edit("unsent draft");f.replacement()
        assertTrue(f.backend.calls.isEmpty());assertTrue(f.controller.state.value.canRetry)
        assertEquals(listOf("one"),f.controller.state.value.rows.map{it.reference})
        f.controller.retry();assertEquals("accepted",f.backend.calls.last().query);assertNull(f.backend.calls.last().cursor)
        f.backend.calls.last().answer.complete(page("fresh"))
        assertEquals(listOf("fresh"),f.controller.state.value.rows.map{it.reference});assertFalse(f.controller.state.value.canRetry)
        assertEquals("unsent draft",f.controller.state.value.draftQuery)
    }
    @Test fun initialOwnerUnavailableHasNoDeadRetryAndReconnectRestoresIdle()=fixture{f->
        f.controller.availability(false);assertFalse(f.controller.state.value.canRetry)
        f.controller.availability(true)
        assertEquals(RegistrySearchPhase.Idle,f.controller.state.value.phase);assertFalse(f.controller.state.value.canRetry)
        f.controller.retry();assertTrue(f.backend.calls.isEmpty())
    }
    @Test fun backendCurrentCheckRejectsReplyEvenBeforeDisconnectNotification()=fixture{f->
        f.controller.search("q");f.backend.current=false
        f.backend.calls.last().answer.complete(page("stale"))
        assertEquals(RegistrySearchPhase.Unavailable,f.controller.state.value.phase)
        assertFalse(f.controller.state.value.ownerConnected)
        assertTrue(f.controller.state.value.rows.isEmpty());assertTrue(f.failures.isEmpty())
    }
    @Test fun anOldNextPageCannotAppendIntoANewerQuery()=fixture{f->
        f.controller.search("old");f.backend.calls.last().answer.complete(page("old-first",cursor="old-next"))
        f.controller.next();val oldNext=f.backend.calls.last()
        f.controller.search("new");f.backend.calls.last().answer.complete(page("new-first",cursor="new-next"))
        oldNext.answer.complete(page("old-late"))
        assertEquals("new",f.controller.state.value.resultsQuery)
        assertEquals(listOf("new-first"),f.controller.state.value.rows.map{it.reference})
        assertEquals("new-next",f.controller.state.value.nextCursor)
    }
    @Test fun closingClearsPrivateSearchAndPreventsLateAdmission()=fixture{f->
        f.controller.search("q");val old=f.backend.calls.last()
        f.controller.close();old.answer.complete(page("stale"));f.controller.search("denied")
        assertEquals("",f.controller.state.value.draftQuery);assertNull(f.controller.state.value.resultsQuery)
        assertTrue(f.controller.state.value.rows.isEmpty());assertEquals(1,f.backend.calls.size)
    }
    @Test fun cursorCyclesAreRejectedWithPreviousCursorAndRowsIntact()=fixture{f->
        f.controller.search("q");f.backend.calls.last().answer.complete(page("one",cursor="a"))
        f.controller.next();f.backend.calls.last().answer.complete(page("two",cursor="b"))
        f.controller.next();f.backend.calls.last().answer.complete(page("three",cursor="a"))
        assertEquals(RegistrySearchProblem.InvalidPage,f.controller.state.value.problem)
        assertEquals(listOf("one","two"),f.controller.state.value.rows.map{it.reference});assertEquals("b",f.controller.state.value.nextCursor)
    }
    @Test fun pageAndCursorLimitsRejectInsteadOfClaimingEndOfResults()=fixture{f->
        f.controller.search("q");f.backend.calls.last().answer.complete(RegistrySearchPage(List(31){row("$it")},null))
        assertEquals(RegistrySearchProblem.InvalidPage,f.controller.state.value.problem);assertFalse(f.controller.state.value.empty)
        f.controller.search("q");f.backend.calls.last().answer.complete(page("one",cursor="x".repeat(4097)))
        assertEquals(RegistrySearchProblem.InvalidPage,f.controller.state.value.problem);assertTrue(f.controller.state.value.rows.isEmpty())
    }
    @Test fun totalRowBudgetRejectsOnlyOverflowPageAndPreservesAcceptedPages()=fixture{f->
        f.controller.search("q")
        repeat(8){index->
            f.backend.calls.last().answer.complete(RegistrySearchPage(List(30){row("${index*30+it}")},"page$index"))
            f.controller.next()
        }
        f.backend.calls.last().answer.complete(page("overflow"))
        assertEquals(240,f.controller.state.value.rows.size);assertEquals(RegistrySearchProblem.LimitReached,f.controller.state.value.problem)
        assertEquals("page7",f.controller.state.value.nextCursor);assertFalse(f.controller.state.value.empty)
    }
    @Test fun pageBudgetBoundsDuplicateOnlyPaginationWithoutPretendingCompletion()=fixture{f->
        f.controller.search("q")
        repeat(32){index->f.backend.calls.last().answer.complete(page("same",cursor="page$index"));f.controller.next()}
        assertEquals(32,f.backend.calls.size);assertEquals(32,f.controller.state.value.pages)
        assertEquals(RegistrySearchProblem.LimitReached,f.controller.state.value.problem);assertEquals("page31",f.controller.state.value.nextCursor)
    }
    @Test fun perRowAndRetainedByteBudgetsRejectWithoutDiscardingGoodResults()=fixture{f->
        f.controller.search("q");f.backend.calls.last().answer.complete(RegistrySearchPage(listOf(row("huge","x".repeat(16384))),null))
        assertEquals(RegistrySearchProblem.InvalidPage,f.controller.state.value.problem)
        f.controller.search("q")
        repeat(2){index->
            f.backend.calls.last().answer.complete(RegistrySearchPage(List(30){row("$index-$it","x".repeat(15000))},"page$index"))
            f.controller.next()
        }
        f.backend.calls.last().answer.complete(RegistrySearchPage(List(30){row("2-$it","x".repeat(15000))},null))
        assertEquals(RegistrySearchProblem.LimitReached,f.controller.state.value.problem)
        assertEquals(60,f.controller.state.value.rows.size);assertEquals("page1",f.controller.state.value.nextCursor)
    }
    @Test fun unknownOwnerIsNotAnInternetClaimAndInvalidInputDoesNotFetch()=fixture{f->
        f.controller.availability(false);f.controller.search("q")
        assertTrue(f.backend.calls.isEmpty());assertFalse(f.controller.state.value.ownerConnected)
        f.controller.availability(true);assertTrue(f.backend.calls.isEmpty())
        f.controller.search(" ");f.controller.search("x".repeat(257))
        assertEquals(RegistrySearchProblem.InvalidQuery,f.controller.state.value.problem);assertTrue(f.backend.calls.isEmpty())
    }
}
