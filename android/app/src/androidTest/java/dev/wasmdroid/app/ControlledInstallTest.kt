package dev.wasmdroid.app

import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import dev.wasmdroid.engine.*
import org.json.JSONArray
import org.json.JSONObject
import org.junit.Assert.*
import org.junit.Test
import org.junit.runner.RunWith
import java.io.File
import java.util.UUID

/** Actual registry/native install cancellation, isolated from application data.
 * Not a service execution, background/GUI or total-cache policy test. */
@RunWith(AndroidJUnit4::class)
class ControlledInstallTest{
    @Test fun cancelAfterBodyProgressPublishesNoPartialImageMetadata(){
        val context=InstrumentationRegistry.getInstrumentation().targetContext
        val base=context.cacheDir.canonicalFile
        val root=File(base,"test-controlled-install-${UUID.randomUUID()}").canonicalFile
        check(root.parentFile==base&&root.name.startsWith("test-controlled-install-"))
        root.mkdirs()
        val control=InstallControl();var storage:EngineStorage?=null;var accepted=false
        try{
            val engine=EngineStorage.open(root.absolutePath);storage=engine
            var failure:Exception?=null
            try{engine.pullImageControlled("wasmer.io/python/python@3.13.20",512uL*1024uL*1024uL,null,control,object:ProbeObserver{
                override fun onPhase(phase:String){
                    val event=JSONObject(phase)
                    if(event.optString("phase")=="downloading"&&event.optLong("bytes_downloaded")>0&&!accepted)accepted=control.cancel()
                }
            })}catch(error:Exception){failure=error}
            assertTrue("No body progress cancellation was accepted",accepted)
            assertNotNull(failure);assertTrue(failure!!.message.orEmpty().contains("operation cancelled"))
            assertEquals(InstallStatus.CANCELLED,control.status())
            assertEquals(0,JSONArray(engine.listImages()).length())
            val blobs=File(root,"store/blobs/sha256").listFiles().orEmpty()
            assertFalse(blobs.any{it.name.startsWith("wasmdroid-download-")})
        }finally{
            storage?.close();control.close()
            // Only this uniquely generated cache fixture, after owner release.
            check(root.canonicalFile.parentFile==base&&root.name.startsWith("test-controlled-install-"))
            root.deleteRecursively()
        }
    }
}
