package dev.wasmdroid.app

import android.app.Application
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import dev.wasmdroid.app.ui.*
import kotlinx.coroutines.*
import org.junit.Assert.*
import org.junit.Test
import org.junit.runner.RunWith
import java.io.ByteArrayInputStream

/** Bounded document/control regressions, not provider/Compose/runtime acceptance. */
@RunWith(AndroidJUnit4::class)
class StackImportTest{
    @Test fun documentBoundaryCountsBytesAndRejectsMalformedUtf8WithoutReplacement(){
        val text="é".repeat(MAX_STACK_DOCUMENT_BYTES/2)
        assertEquals(text,readStackDocument(ByteArrayInputStream(text.toByteArray(Charsets.UTF_8))))
        assertEquals("services: {}",readStackDocument(ByteArrayInputStream("\uFEFFservices: {}".toByteArray(Charsets.UTF_8))))
        val oversized=ByteArrayInputStream(ByteArray(MAX_STACK_DOCUMENT_BYTES+8192){65})
        try{readStackDocument(oversized);fail("oversize accepted")}catch(expected:StackDocumentTooLarge){}
        assertEquals(8191,oversized.available()) // Admission reads at most the limit plus one byte.
        for(bytes in listOf(byteArrayOf(0xc3.toByte()),byteArrayOf(0xc0.toByte(),0xaf.toByte()),byteArrayOf(0xed.toByte(),0xa0.toByte(),0x80.toByte()))){
            try{readStackDocument(ByteArrayInputStream(bytes));fail("malformed UTF-8 accepted")}
            catch(expected:java.nio.charset.CharacterCodingException){}
        }
    }

    @Test fun stalePreviewCannotAuthorizeAnEditedDraftAndCreatePinsTheReviewedInput()=runBlocking{
        val application=InstrumentationRegistry.getInstrumentation().targetContext.applicationContext as Application
        val scope=CoroutineScope(SupervisorJob()+Dispatchers.Unconfined)
        val late=CompletableDeferred<String>();val commit=CompletableDeferred<Unit>()
        val previews=mutableListOf<Pair<String,String>>();val creates=mutableListOf<Pair<String,String>>()
        var created:String?=null
        val backend=object:StackImportBackend{
            override suspend fun requirements(document:String)="[]"
            override suspend fun preview(id:String,document:String):String{
                previews.add(id to document)
                return if(previews.size==1)withContext(NonCancellable){late.await()}else "{\"reviewed\":true}"
            }
            override suspend fun create(id:String,document:String,preview:String){assertEquals("{\"reviewed\":true}",preview);creates.add(id to document);commit.await()}
        }
        val controller=StackImportController(application,scope,backend,{created=it})
        try{
            controller.edit(id="first",document="old")
            controller.preview();assertTrue(controller.state.value.busy)
            controller.edit(id="second",document="new")
            late.complete("{\"stale\":true}");yield()
            assertNull(controller.state.value.preview);assertFalse(controller.state.value.busy)
            controller.create();assertTrue(creates.isEmpty())
            controller.preview();assertNotNull(controller.state.value.preview)
            controller.create();assertEquals(listOf("second" to "new"),creates)
            controller.edit(document="changed during commit");controller.clear()
            assertEquals("new",controller.state.value.document)
            commit.complete(Unit);yield()
            assertEquals("second",created);assertEquals("second",controller.state.value.createdId)
            assertEquals("",controller.state.value.document);assertNull(controller.state.value.preview)
            controller.clear();assertNull(controller.state.value.createdId)
        }finally{late.complete("{}");commit.complete(Unit);controller.close();scope.cancel()}
    }
}
