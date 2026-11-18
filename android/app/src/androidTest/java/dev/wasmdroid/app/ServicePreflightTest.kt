package dev.wasmdroid.app

import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import dev.wasmdroid.engine.EngineStorage
import org.json.JSONArray
import org.json.JSONObject
import org.junit.Assert.*
import org.junit.Test
import org.junit.runner.RunWith
import java.io.File
import java.util.UUID

/** Native pure validation only; no download, dataset preparation or guest. */
@RunWith(AndroidJUnit4::class)
class ServicePreflightTest{
    @Test fun unsupportedResourceRecipeFailsBeforeAnyDatasetOrInstanceIsPrepared(){
        val base=InstrumentationRegistry.getInstrumentation().targetContext.cacheDir.canonicalFile
        val root=File(base,"test-service-preflight-${UUID.randomUUID()}").canonicalFile
        check(root.parentFile==base&&root.name.startsWith("test-service-preflight-"));root.mkdirs()
        var storage:EngineStorage?=null
        try{
            val engine=EngineStorage.open(root.absolutePath);storage=engine
            val spec=JSONObject().put("artifact_ref","wasmer.io/python/python@3.13.20").put("command","python")
                .put("mounts",JSONArray().put(JSONObject().put("source",JSONObject().put("type","volume").put("source","unprepared")).put("target","/data").put("read_only",false)))
            for(policy in listOf("no","on_failure","always")){
                spec.put("restart_policy",policy).put("start_on_boot",true)
                engine.validateServiceSpec(spec.toString())
            }
            for(resource in listOf("memory_max_bytes","cpu_shares")){
                spec.put("resources",JSONObject().put(resource,1024*1024))
                var failed=false
                try{engine.validateServiceSpec(spec.toString())}catch(expected:Exception){failed=true}
                assertTrue("Unenforced $resource must remain unsupported",failed)
            }
            assertEquals(0,JSONArray(engine.listInstances("local")).length());assertEquals(0,JSONArray(engine.listVolumes()).length());assertEquals(0,JSONArray(engine.listImages()).length())
        }finally{
            storage?.close();check(root.canonicalFile.parentFile==base&&root.name.startsWith("test-service-preflight-"));root.deleteRecursively()
        }
    }
}
