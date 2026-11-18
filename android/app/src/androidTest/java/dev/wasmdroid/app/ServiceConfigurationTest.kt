package dev.wasmdroid.app

import android.content.Context
import android.content.ContextWrapper
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import dev.wasmdroid.app.engine.LocalPackageInput
import dev.wasmdroid.app.engine.LocalPackageMetadata
import dev.wasmdroid.app.engine.ServiceSupervisor
import dev.wasmdroid.app.ui.LocalPackageView
import dev.wasmdroid.engine.EngineStorage
import dev.wasmdroid.engine.InstallControl
import org.json.JSONArray
import org.json.JSONObject
import org.junit.Assert.*
import org.junit.Test
import org.junit.runner.RunWith
import java.io.ByteArrayInputStream
import java.io.File
import java.util.UUID
import java.util.concurrent.TimeUnit

/** Actual native metadata/preflight and common-owner data fencing only; no
 * guest, provider UI, worker-death or rendered-form acceptance is implied. */
@RunWith(AndroidJUnit4::class)
class ServiceConfigurationTest {
    private val wasm=byteArrayOf(0,97,115,109,1,0,0,0,1,4,1,96,0,0,3,2,1,0,
        7,10,1,6,95,115,116,97,114,116,0,0,10,4,1,2,0,11)
    @Test fun explicitNewVolumeRejectsExistingDataWithoutReplacingIt()=isolated{root->
        EngineStorage.open(File(root,"engine").absolutePath).use{db->
            db.createNewVolume("dataset")
            val file=File(root,"engine/volumes/data/dataset/kept.txt")
            file.writeText("retained bytes")
            val previous=db.listVolumes()
            rejects{db.createNewVolume("dataset")}
            assertEquals(previous,db.listVolumes())
            assertEquals("retained bytes",file.readText())
        }
    }
    @Test fun nativeConfigurePreservesIdentityAndRejectsUnsupportedOrStaleWritesBeforeMutation()=isolated{root->
        EngineStorage.open(File(root,"engine").absolutePath).use{db->
            val reference=db.beginPackageImport(wasm.size.toULong(),4096uL,null,InstallControl()).use{transfer->
                transfer.write(wasm);val preview=JSONObject(transfer.preview());JSONObject(transfer.commit(preview.getString("token"))).getString("reference")
            }
            db.createVolume("old");db.createVolume("new")
            val spec=JSONObject().put("artifact_ref",reference).put("command","main").put("env",JSONObject().put("PRIVATE_ENV","PRIVATE_VALUE"))
                .put("mounts",JSONArray().put(JSONObject().put("source",JSONObject().put("type","volume").put("source","old")).put("target","/data").put("read_only",false)))
            val created=JSONObject(db.createInstance("owned-app",spec.toString()))
            spec.getJSONArray("mounts").getJSONObject(0).getJSONObject("source").put("source","new")
            val configured=JSONObject(db.configureInstance("owned-app",created.getLong("version").toULong(),spec.toString()))
            val revision=configured.getLong("version").toULong()
            assertEquals("created",configured.getString("state"));assertEquals("owned-app",configured.getString("id"))
            assertEquals(reference,configured.getJSONObject("spec").getString("artifact_ref"))
            rejects{db.configureInstance("owned-app",created.getLong("version").toULong(),spec.toString())}
            val unsupported=JSONObject(spec.toString()).put("resources",JSONObject().put("memory_max_bytes",1024))
            rejects{db.configureInstance("owned-app",revision,unsupported.toString())}
            val invalid=JSONObject(spec.toString()).put("PRIVATE_FIELD_MARKER",true)
            val error=rejects{db.configureInstance("owned-app",revision,invalid.toString())}
            assertFalse(error.message.orEmpty().contains("PRIVATE_FIELD_MARKER"))
            val current=JSONObject(db.inspectInstance("owned-app"))
            assertEquals(revision,current.getLong("version").toULong())
            assertEquals("new",current.getJSONObject("spec").getJSONArray("mounts").getJSONObject(0).getJSONObject("source").getString("source"))
            assertEquals(2,JSONArray(db.listVolumes()).length())
            assertFalse(db.eventsAfter(0uL,100u).contains("PRIVATE_"))
        }
    }
    @Test fun commonActorRefusesConfigurationWhileCreatedInstanceHasADataOwner()=isolated{root->
        val target=InstrumentationRegistry.getInstrumentation().targetContext
        val scoped=object:ContextWrapper(target){override fun getApplicationContext():Context=this;override fun getFilesDir():File=root}
        val activity=java.util.concurrent.atomic.AtomicReference<ServiceSupervisor.Activity?>()
        val owner=ServiceSupervisor(scoped){activity.set(it)}
        try{
            val stream=ByteArrayInputStream(wasm)
            val input=object:LocalPackageInput{
                override fun open()=LocalPackageMetadata("fixture.wasm",wasm.size.toLong())
                override fun read(buffer:ByteArray)=stream.read(buffer)
                override fun close(){stream.close()}
                override fun cancel(){stream.close()}
            }
            val handle=JSONObject(owner.importPackageInput(input).get(10,TimeUnit.SECONDS))
            val key=handle.getString("key");val generation=handle.getString("generation")
            val preview=await(owner,key,generation,"awaiting_review").preview!!
            owner.commitLocalPackage(key,generation,preview.token).get(10,TimeUnit.SECONDS)
            await(owner,key,generation,"installed")
            val created=JSONObject(owner.create("owned-app",preview.spec("main")).get(10,TimeUnit.SECONDS))
            val revision=created.getLong("version").toULong()
            val next=JSONObject(created.getJSONObject("spec").toString()).put("args",JSONArray().put("new argument"))
            val lease=owner.beginDataOperation("owned-app").get(10,TimeUnit.SECONDS)
            try{
                val summary=owner.notificationSummary().get(10,TimeUnit.SECONDS)
                assertTrue(summary.backgroundWork);assertEquals(0,summary.totalApplications)
                assertEquals(0,owner.activeCount().get(10,TimeUnit.SECONDS))
                assertTrue(activity.get()!!.summary.backgroundWork)
                assertEquals(0,activity.get()!!.ownedCount)
                assertNull(owner.requestIdleRetirement().get(10,TimeUnit.SECONDS))
                rejects{owner.configure("owned-app",revision,next.toString()).get(10,TimeUnit.SECONDS)}
            }finally{lease.close()}
            assertFalse(owner.notificationSummary().get(10,TimeUnit.SECONDS).backgroundWork)
            assertEquals(revision,JSONObject(owner.inspect("owned-app").get(10,TimeUnit.SECONDS)).getLong("version").toULong())
            val updated=JSONObject(owner.configure("owned-app",revision,next.toString()).get(10,TimeUnit.SECONDS))
            assertEquals("created",updated.getString("state"))
            assertEquals(revision+1uL,updated.getLong("version").toULong())
            assertEquals(0,JSONObject(owner.controllers().get(10,TimeUnit.SECONDS)).length())
            // A locked native database must not park the controller actor or
            // permit launching this instance while its edit is still pending.
            val blocked=android.database.sqlite.SQLiteDatabase.openDatabase(File(root,"engine/engine.sqlite").absolutePath,null,android.database.sqlite.SQLiteDatabase.OPEN_READWRITE)
            blocked.beginTransaction()
            val pending=owner.configure("owned-app",updated.getLong("version").toULong(),JSONObject(next.toString()).put("args",JSONArray().put("another argument")).toString())
            try{
                val controllers=JSONObject(owner.controllers().get(3,TimeUnit.SECONDS))
                assertEquals("data",controllers.getString("owned-app"))
                assertFalse(pending.isDone)
                rejects{owner.start("owned-app").get(3,TimeUnit.SECONDS)}
            }finally{blocked.endTransaction();blocked.close()}
            assertEquals("created",JSONObject(pending.get(20,TimeUnit.SECONDS)).getString("state"))
        }finally{owner.close().get(25,TimeUnit.SECONDS)}
    }
    @Test fun configurationKeepsPreviouslyGrantedNetworkPermissions() = isolated { root ->
        EngineStorage.open(File(root, "engine").absolutePath).use { db ->
            val reference = db.beginPackageImport(wasm.size.toULong(), 4096uL, null, InstallControl()).use { transfer ->
                transfer.write(wasm)
                val preview = JSONObject(transfer.preview())
                JSONObject(transfer.commit(preview.getString("token"))).getString("reference")
            }
            val id = "networked-app"
            val initial = JSONObject().put("artifact_ref", reference).put("command", "main")
            val created = JSONObject(db.createInstance(id, initial.toString()))
            val oldVersion = created.getLong("version").toULong()
            val source = JSONObject(db.networkPermissionSource(id))
            val permitted = JSONObject(db.setNetworkPermissions(id, source.getString("identity"), oldVersion, true, false))
            val permittedVersion = permitted.getLong("version").toULong()
            val edited = permitted.getJSONObject("spec")
                .put("ports", JSONArray().put(JSONObject().put("instance_port", 8083).put("host_port", 0).put("protocol", "tcp")))
                .put("labels", JSONObject().put("dev.wasmdroid.startup.port", "8083"))
            rejects { db.configureInstance(id, oldVersion, edited.toString()) }
            val configured = JSONObject(db.configureInstance(id, permittedVersion, edited.toString()))
            val savedSpec = configured.getJSONObject("spec")
            assertTrue(savedSpec.getBoolean("internet_access"))
            assertFalse(savedSpec.optBoolean("lan_access"))
            assertEquals(8083, savedSpec.getJSONArray("ports").getJSONObject(0).getInt("instance_port"))
            val version = configured.getLong("version").toULong()
            rejects { db.configureInstance(id, version, JSONObject(edited.toString()).put("internet_access", false).toString()) }
            rejects { db.configureInstance(id, version, JSONObject(edited.toString()).put("lan_access", true).toString()) }
            val current = JSONObject(db.inspectInstance(id))
            assertEquals(version, current.getLong("version").toULong())
            assertTrue(current.getJSONObject("spec").getBoolean("internet_access"))
            assertFalse(current.getJSONObject("spec").optBoolean("lan_access"))
        }
    }
    @Test fun staleBrowserDataConfirmationCannotClearARecreatedApplicationWithTheSameId()=isolated{root->
        val spec=EngineStorage.open(File(root,"engine").absolutePath).use{db->
            val reference=db.beginPackageImport(wasm.size.toULong(),4096uL,null,InstallControl()).use{transfer->
                transfer.write(wasm);val preview=JSONObject(transfer.preview())
                JSONObject(transfer.commit(preview.getString("token"))).getString("reference")
            }
            JSONObject().put("artifact_ref",reference).put("command","main").toString().also{
                db.createInstance("browser-alias",it)
            }
        }
        val target=InstrumentationRegistry.getInstrumentation().targetContext
        val scoped=object:ContextWrapper(target){override fun getApplicationContext():Context=this;override fun getFilesDir():File=root}
        val owner=ServiceSupervisor(scoped)
        try{
            val prior=JSONObject(owner.browserProfileState("browser-alias").get(10,TimeUnit.SECONDS))
            owner.remove("browser-alias").get(10,TimeUnit.SECONDS)
            owner.create("browser-alias",spec).get(10,TimeUnit.SECONDS)
            val current=JSONObject(owner.browserProfileState("browser-alias").get(10,TimeUnit.SECONDS))
            assertNotEquals(prior.getString("profile_token"),current.getString("profile_token"))
            assertEquals(prior.getLong("revision"),current.getLong("revision"))
            val failure=rejects{
                owner.eraseBrowserProfile("browser-alias",prior.getLong("revision"),prior.getString("profile_token")).get(10,TimeUnit.SECONDS)
            }
            var cause:Throwable=failure;repeat(8){cause.cause?.let{cause=it}}
            assertEquals("Browser profile changed; review its data again",cause.message)
            val after=JSONObject(owner.browserProfileState("browser-alias").get(10,TimeUnit.SECONDS))
            assertEquals(current.getString("profile_token"),after.getString("profile_token"))
            assertEquals(current.getLong("revision"),after.getLong("revision"))
            assertEquals("preserved",after.getString("status"));assertFalse(after.getBoolean("pending_erasure"))
        }finally{owner.close().get(25,TimeUnit.SECONDS)}
    }
    @Test fun coldStoppedShortcutRetainsItsStateFenceAndLateSelectionChangeCannotStartIt()=isolated{root->
        EngineStorage.open(File(root,"engine").absolutePath).use{db->
            val reference=db.beginPackageImport(wasm.size.toULong(),4096uL,null,InstallControl()).use{transfer->
                transfer.write(wasm);val preview=JSONObject(transfer.preview());JSONObject(transfer.commit(preview.getString("token"))).getString("reference")
            }
            db.createInstance("cold-app",JSONObject().put("artifact_ref",reference).put("command","main").toString())
        }
        val target=InstrumentationRegistry.getInstrumentation().targetContext
        val scoped=object:ContextWrapper(target){override fun getApplicationContext():Context=this;override fun getFilesDir():File=root}
        val first=ServiceSupervisor(scoped)
        val record=try{JSONObject(first.applicationTarget("application","cold-app").get(10,TimeUnit.SECONDS))}
        finally{first.close().get(25,TimeUnit.SECONDS)}
        val second=ServiceSupervisor(scoped)
        try{
            val key=record.getJSONObject("target").toString()
            val current=JSONObject(second.shortcutResolve(key).get(10,TimeUnit.SECONDS))
            assertEquals("stopped",current.getString("state"));assertTrue(current.getBoolean("can_start"))
            assertEquals(record.getString("state_token"),current.getString("state_token"))
            val checks=java.util.concurrent.atomic.AtomicInteger()
            rejects{second.shortcutAction(key,"start",current.getString("state_token")){checks.incrementAndGet()==1}.get(10,TimeUnit.SECONDS)}
            assertEquals(2,checks.get())
            val unchanged=JSONObject(second.inspect("cold-app").get(10,TimeUnit.SECONDS))
            assertEquals("created",unchanged.getString("state"));assertEquals(0L,unchanged.getLong("version"))
            assertEquals(0,JSONObject(second.controllers().get(10,TimeUnit.SECONDS)).length())
        }finally{second.close().get(25,TimeUnit.SECONDS)}
    }
    private fun await(owner:ServiceSupervisor,key:String,generation:String,status:String):LocalPackageView{
        val deadline=System.nanoTime()+TimeUnit.SECONDS.toNanos(20)
        while(System.nanoTime()<deadline){
            val view=LocalPackageView.parse(owner.localPackage(key,generation).get(5,TimeUnit.SECONDS))
            if(view.status==status&&!view.busy)return view
            check(view.status !in setOf("failed","cancelled")){"Unexpected import outcome"}
            Thread.sleep(20)
        }
        error("Import outcome timed out")
    }
    private fun rejects(block:()->Unit):Exception{
        try{block()}catch(expected:Exception){return expected}
        throw AssertionError("Configuration must be rejected")
    }
    private fun isolated(block:(File)->Unit){
        val base=InstrumentationRegistry.getInstrumentation().targetContext.cacheDir.canonicalFile
        val root=File(base,"test-configure-${UUID.randomUUID()}").canonicalFile
        check(root.parentFile==base&&root.name.startsWith("test-configure-"));root.mkdirs()
        try{block(root)}finally{
            check(root.canonicalFile.parentFile==base&&root.name.startsWith("test-configure-"));root.deleteRecursively()
        }
    }
}
