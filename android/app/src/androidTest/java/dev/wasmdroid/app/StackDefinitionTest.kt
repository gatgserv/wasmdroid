package dev.wasmdroid.app

import android.content.*
import android.os.IBinder
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import dev.wasmdroid.app.engine.EngineService
import dev.wasmdroid.app.engine.ServiceSupervisor
import org.json.JSONArray
import org.json.JSONObject
import org.junit.Assert.*
import org.junit.Test
import org.junit.runner.RunWith
import java.util.UUID
import java.util.concurrent.CompletableFuture
import java.util.concurrent.TimeUnit

/** Live package metadata + native stack storage only. Does not establish that
 * stack runtime routing, dependency startup or any guest behavior works. */
@RunWith(AndroidJUnit4::class)
class StackDefinitionTest {
    @Test fun nativeDefinitionPlansCreatesAndRemovesOnlyOwnedMetadataPreservingDatasets() {
        val context=InstrumentationRegistry.getInstrumentation().targetContext
        val ready=CompletableFuture<ServiceSupervisor>()
        val connection=object:ServiceConnection {
            override fun onServiceConnected(name:ComponentName?,service:IBinder?){ready.complete((service as EngineService.LocalBinder).engine)}
            override fun onServiceDisconnected(name:ComponentName?){}
        }
        assertTrue(context.bindService(Intent(context,EngineService::class.java),connection,Context.BIND_AUTO_CREATE))
        val id="test-stack-${UUID.randomUUID()}";var engine:ServiceSupervisor?=null;var createdVersion:ULong?=null
        val volumes=mutableListOf<String>()
        try {
            val supervisor=ready.get(20,TimeUnit.SECONDS);engine=supervisor
            supervisor.pull("wasmer.io/python/python@3.13.20").get(300,TimeUnit.SECONDS)
            val document="""
                services:
                  db:
                    image: wasmer.io/python/python@3.13.20
                    command: [python, '-V']
                    volumes: ['data:/data']
                  web:
                    image: wasmer.io/python/python@3.13.20
                    command: [python, '-V']
                    depends_on: [db]
                    environment: {PRIVATE_VALUE: 'synthetic-private-marker'}
                volumes: {data: {}}
            """.trimIndent()
            val preview=JSONObject(supervisor.previewStack(id,document).get(20,TimeUnit.SECONDS))
            assertEquals("db",preview.getJSONArray("startup_order").getString(0))
            val record=JSONObject(supervisor.createStack(id,document).get(20,TimeUnit.SECONDS));var version=record.getLong("version").toULong();createdVersion=version
            val declared=record.getJSONObject("plan").getJSONObject("volumes")
            declared.keys().forEach{name->volumes.add(declared.getJSONObject(name).getString("name"))}
            val members=JSONArray(supervisor.stackMembers(id).get(10,TimeUnit.SECONDS));assertEquals(2,members.length())
            for(index in 0 until members.length())assertEquals("created",members.getJSONObject(index).getString("state"))
            val source=JSONObject(supervisor.stackEditSource(id).get(20,TimeUnit.SECONDS))
            val edited=JSONObject(source.getString("document"))
            edited.getJSONObject("services").getJSONObject("web").getJSONObject("environment").put("PRIVATE_VALUE","updated-private-marker")
            val update=supervisor.previewStackUpdate(id,version,edited.toString()).get(20,TimeUnit.SECONDS)
            val lease=supervisor.beginDataOperation(members.getJSONObject(0).getString("instance_id")).get(10,TimeUnit.SECONDS)
            try{
                var refused=false
                try{supervisor.updateReviewedStack(id,update).get(20,TimeUnit.SECONDS)}catch(_:java.util.concurrent.ExecutionException){refused=true}
                assertTrue("A live data lease must fence the entire stack edit",refused)
                assertEquals(version,JSONObject(supervisor.inspectStack(id).get(10,TimeUnit.SECONDS)).getLong("version").toULong())
            }finally{lease.close()}
            val applied=JSONObject(supervisor.updateReviewedStack(id,update).get(20,TimeUnit.SECONDS))
            val previous=version;version=applied.getLong("version").toULong();createdVersion=version
            assertTrue(version>previous)
            val controllers=JSONObject(supervisor.controllers().get(10,TimeUnit.SECONDS))
            for(index in 0 until members.length())assertFalse(controllers.has(members.getJSONObject(index).getString("instance_id")))
            assertEquals(members.getJSONObject(0).getString("instance_id"),applied.getJSONArray("instances").getJSONObject(0).getString("id"))
            var staleRefused=false
            try{supervisor.updateReviewedStack(id,update).get(20,TimeUnit.SECONDS)}catch(_:java.util.concurrent.ExecutionException){staleRefused=true}
            assertTrue("A reviewed update must not be replayed at a newer version",staleRefused)
            assertEquals(id,JSONObject(supervisor.inspectStack(id).get(10,TimeUnit.SECONDS)).getString("id"))
            val summaries=supervisor.stacks().get(10,TimeUnit.SECONDS)
            assertFalse(summaries.contains("synthetic-private-marker"))
            assertFalse(summaries.contains("updated-private-marker"))
            supervisor.removeStack(id,version).get(20,TimeUnit.SECONDS);createdVersion=null
            val retained=JSONArray(supervisor.volumes().get(10,TimeUnit.SECONDS))
            for(volume in volumes)assertTrue((0 until retained.length()).any{retained.getJSONObject(it).getString("name")==volume})
        } finally {
            try{engine?.let{supervisor->
                createdVersion?.let{version->supervisor.removeStack(id,version).get(20,TimeUnit.SECONDS)}
                // Only datasets allocated by this uniquely named test definition.
                for(volume in volumes)supervisor.removeVolume(volume).get(10,TimeUnit.SECONDS)
            }} finally {context.unbindService(connection)}
        }
    }
}
