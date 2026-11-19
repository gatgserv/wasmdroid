package dev.wasmdroid.app

import android.content.Context
import android.content.ContextWrapper
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import dev.wasmdroid.app.engine.LocalPackageInput
import dev.wasmdroid.app.engine.LocalPackageMetadata
import dev.wasmdroid.app.engine.ServiceSupervisor
import dev.wasmdroid.app.ui.AppState
import dev.wasmdroid.app.ui.LocalPackageView
import dev.wasmdroid.app.ui.packageTasks
import dev.wasmdroid.app.ui.packageWorkActive
import dev.wasmdroid.engine.EngineStorage
import org.json.JSONArray
import org.json.JSONObject
import org.junit.Assert.*
import org.junit.Test
import org.junit.runner.RunWith
import java.io.ByteArrayInputStream
import java.io.File
import java.util.UUID
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicBoolean

/** Actual native/common-owner boundary in an isolated root. The injected stream
 * tests uncooperative provider IO; real SAF/rotation/visual/worker tests are separate. */
@RunWith(AndroidJUnit4::class)
class LocalPackageOwnerTest {
    private class Input(bytes:ByteArray,private val hold:Boolean=false,private val fail:Boolean=false):LocalPackageInput {
        private val stream=ByteArrayInputStream(bytes)
        val entered=CountDownLatch(1);val release=CountDownLatch(1)
        val closed=AtomicBoolean(false);private val cancelled=AtomicBoolean(false)
        override fun open()=LocalPackageMetadata("owned-fixture.wasm",null)
        override fun read(buffer:ByteArray):Int {
            entered.countDown()
            if(hold)check(release.await(20,TimeUnit.SECONDS))
            if(cancelled.get())error("Provider cancelled")
            if(fail)error("content://private/document?token=DO_NOT_ECHO")
            return stream.read(buffer)
        }
        override fun cancel(){cancelled.set(true)}
        override fun close(){closed.set(true);stream.close()}
    }
    private fun wasm():ByteArray = byteArrayOf(0,97,115,109,1,0,0,0,1,4,1,96,0,0,3,2,1,0,
        7,10,1,6,95,115,116,97,114,116,0,0,10,4,1,2,0,11)+
        byteArrayOf(0,38,1,120)+UUID.randomUUID().toString().toByteArray(Charsets.US_ASCII)

    @Test fun acceptedCancelAndOwnerCloseWaitForActualReadCleanup() = isolated {owner,root->
        val input=Input(wasm(),hold=true)
        val handle=JSONObject(owner.importPackageInput(input).get(10,TimeUnit.SECONDS))
        try {
            assertTrue(input.entered.await(10,TimeUnit.SECONDS))
            assertTrue(owner.cancelPackageOperation(handle.getString("key"),handle.getString("generation")))
            val rows=packageTasks(owner.packageActivity().get(10,TimeUnit.SECONDS))
            val pending=rows.single()
            assertFalse(pending.terminal);assertEquals("cancelled",pending.status)
            val closing=owner.close()
            assertFalse(closing.isDone)
            input.release.countDown()
            closing.get(20,TimeUnit.SECONDS)
            assertTrue(input.closed.get())
            EngineStorage.open(File(root,"engine").absolutePath).use{db->
                assertEquals(0,JSONArray(db.listImages()).length())
                assertEquals(0,JSONArray(db.listInstances("local")).length())
            }
        }finally{input.release.countDown()}
    }

    @Test fun previewCanBeReopenedAndExplicitCommitThenCreateNeverStartsAGuest() = isolated {owner,_->
        val input=Input(wasm())
        val handle=JSONObject(owner.importPackageInput(input).get(10,TimeUnit.SECONDS))
        val key=handle.getString("key");val generation=handle.getString("generation")
        val reviewed=awaitFrame(owner,key,generation,"awaiting_review")
        val preview=reviewed.preview!!
        assertTrue(input.closed.get())
        assertEquals(0,JSONArray(owner.images().get(10,TimeUnit.SECONDS)).length())
        assertFalse(AppState(packages=packageTasks(owner.packageActivity().get(10,TimeUnit.SECONDS))).packageWorkActive())
        val reopened=LocalPackageView.parse(owner.localPackage(key,generation).get(10,TimeUnit.SECONDS))
        assertEquals(preview.token,reopened.preview!!.token)
        rejects{owner.commitLocalPackage(key,"stale-generation",preview.token).get(10,TimeUnit.SECONDS)}
        rejects{owner.commitLocalPackage(key,generation,"not-reviewed").get(10,TimeUnit.SECONDS)}
        owner.reviewLocalPackage(key,generation).get(10,TimeUnit.SECONDS)
        val current=awaitFrame(owner,key,generation,"awaiting_review").preview!!
        owner.commitLocalPackage(key,generation,current.token).get(10,TimeUnit.SECONDS)
        val installed=awaitFrame(owner,key,generation,"installed")
        assertFalse(installed.busy)
        assertFalse(owner.cancelPackageOperation(key,generation))
        assertEquals(1,JSONArray(owner.images().get(10,TimeUnit.SECONDS)).length())
        assertEquals(0,JSONArray(owner.instances().get(10,TimeUnit.SECONDS)).length())
        owner.create("owned-app",current.spec("main")).get(10,TimeUnit.SECONDS)
        val created=JSONArray(owner.instances().get(10,TimeUnit.SECONDS)).getJSONObject(0)
        assertEquals("created",created.getString("state"))
        assertEquals(0,JSONObject(owner.controllers().get(10,TimeUnit.SECONDS)).length())
        assertEquals(0,JSONArray(owner.volumes().get(10,TimeUnit.SECONDS)).length())
    }

    @Test fun providerFailureIsFailedAndNeverLeaksTheProviderUriInSnapshots() = isolated {owner,_->
        val input=Input(wasm(),fail=true)
        val handle=JSONObject(owner.importPackageInput(input).get(10,TimeUnit.SECONDS))
        val failed=awaitFrame(owner,handle.getString("key"),handle.getString("generation"),"failed")
        assertTrue(input.closed.get());assertFalse(failed.busy);assertNull(failed.preview)
        val snapshot=owner.packageActivity().get(10,TimeUnit.SECONDS)
        assertFalse(snapshot.contains("DO_NOT_ECHO"));assertFalse(snapshot.contains("content://"))
        val receipt=packageTasks(snapshot).single()
        assertTrue(receipt.terminal);assertEquals("failed",receipt.status);assertFalse(receipt.retryable)
        assertEquals(0,JSONArray(owner.images().get(10,TimeUnit.SECONDS)).length())
    }
    private fun awaitFrame(owner:ServiceSupervisor,key:String,generation:String,status:String):LocalPackageView {
        val deadline=System.nanoTime()+TimeUnit.SECONDS.toNanos(20)
        while(System.nanoTime()<deadline){
            val view=LocalPackageView.parse(owner.localPackage(key,generation).get(5,TimeUnit.SECONDS))
            if(view.status==status&&!view.busy)return view
            if(view.status in setOf("failed","cancelled"))error("Unexpected import outcome: ${view.status}")
            Thread.sleep(20)
        }
        error("Local import did not reach $status")
    }
    private fun rejects(block:()->Unit){var failed=false;try{block()}catch(expected:Exception){failed=true};assertTrue(failed)}
    private fun isolated(block:(ServiceSupervisor,File)->Unit){
        val context=InstrumentationRegistry.getInstrumentation().targetContext
        val base=context.cacheDir.canonicalFile
        val root=File(base,"test-local-owner-${UUID.randomUUID()}").canonicalFile
        check(root.parentFile==base&&root.name.startsWith("test-local-owner-"));root.mkdirs()
        val scoped=object:ContextWrapper(context){override fun getApplicationContext():Context=this;override fun getFilesDir():File=root}
        val owner=ServiceSupervisor(scoped)
        try{block(owner,root)}finally{
            owner.close().get(25,TimeUnit.SECONDS)
            check(root.canonicalFile.parentFile==base&&root.name.startsWith("test-local-owner-"))
            root.deleteRecursively()
        }
    }
}
