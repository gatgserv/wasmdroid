package dev.wasmdroid.app

import dev.wasmdroid.app.ui.CatalogPresentationFrame
import dev.wasmdroid.app.ui.CatalogPublication
import dev.wasmdroid.app.ui.CatalogSelection
import org.junit.Assert.*
import org.junit.Test

/** Pure publication/admission regressions; no Android or image decoder is used. */
class CatalogPresentationTest {
    private val oldEntry="""{"id":"notes","media":{"digest":"same-pixels","credit":"old"}}"""
    private val newEntry="""{"id":"notes","media":{"digest":"same-pixels","credit":"new"}}"""
    private fun frame(source:String?="source-a",entry:String=oldEntry,unavailable:Boolean=false)=
        CatalogPresentationFrame(source,listOf(entry),unavailable)

    @Test fun selectionRequiresAnActualAvailableSourceAndExactEntry() {
        assertNull(frame(null).select(oldEntry))
        assertNull(frame(unavailable=true).select(oldEntry))
        assertNull(frame().select(newEntry))
        assertEquals(CatalogSelection("source-a",oldEntry),frame().select(oldEntry))
    }
    @Test fun lateImageCompletionFromAnOldSourceCannotPublishIdenticalPixels() {
        val selection=frame().select(oldEntry)!!
        val late=CatalogPublication(selection,"decoded-identical-pixels")
        assertEquals("decoded-identical-pixels",late.visible(frame()))
        assertNull(late.visible(frame("source-b")))
    }
    @Test fun samePackageIdAndPixelsCannotRetainOutdatedCreditsOrGrantInstall() {
        val selected=frame().select(oldEntry)!!
        val changed=frame(entry=newEntry)
        assertFalse(changed.accepts(selected))
        assertNull(CatalogPublication(selected,"old-credits").visible(changed))
    }
    @Test fun unavailableOrRemovedSnapshotImmediatelyWithholdsPreviouslyReadyResult() {
        val selected=frame().select(oldEntry)!!
        val ready=CatalogPublication(selected,"ready")
        assertNull(ready.visible(frame(unavailable=true)))
        assertNull(ready.visible(CatalogPresentationFrame("source-a",emptyList(),false)))
        assertNull(ready.visible(frame(null)))
    }
    @Test fun sourceReturningToTheSameBytesNeedsItsNewIncarnationSelection() {
        val old=frame().select(oldEntry)!!
        val returned=frame("source-a-returned")
        assertFalse(returned.accepts(old))
        assertTrue(returned.accepts(returned.select(oldEntry)!!))
    }
    @Test fun cacheContentAndTargetChangesDoNotSupplySelectionAuthority() {
        val old=frame().select(oldEntry)!!
        val another=frame("source-b").select(oldEntry)!!
        assertNull(CatalogPublication(old,"thumbnail").visible(frame("source-b")))
        assertNull(CatalogPublication(old,"full-size").visible(frame("source-b")))
        assertEquals("full-size",CatalogPublication(another,"full-size").visible(frame("source-b")))
    }
}
