package dev.wasmdroid.app

import android.content.*
import android.os.IBinder
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import dev.wasmdroid.app.engine.EngineService
import dev.wasmdroid.app.engine.ServiceSupervisor
import dev.wasmdroid.engine.ProbeObserver
import org.json.JSONObject
import org.junit.Assert.*
import org.junit.Test
import org.junit.runner.RunWith
import java.util.concurrent.*
import java.util.concurrent.atomic.AtomicReference

/** Common-owner live metadata resolution and cancellation, not guest execution.
 * Never removes shared images/data or closes/stops unrelated services. */
@RunWith(AndroidJUnit4::class)
class PackageGenerationTest{
    @Test fun cancelledIsPendingUntilNativeCompletionAndOldGenerationCannotCancelNewRun(){
        val context=InstrumentationRegistry.getInstrumentation().targetContext
        val ready=CompletableFuture<ServiceSupervisor>()
        val connection=object:ServiceConnection{
            override fun onServiceConnected(name:ComponentName?,binder:IBinder?){ready.complete((binder as EngineService.LocalBinder).engine)}
            override fun onServiceDisconnected(name:ComponentName?){}
        }
        assertTrue(context.bindService(Intent(context,EngineService::class.java),connection,Context.BIND_AUTO_CREATE))
        val reference="wasmer.io/python/python@3.13.20";val key="pull:$reference"
        val releases=mutableListOf<CountDownLatch>();val owned=linkedSetOf<String>();val futures=mutableListOf<CompletableFuture<String>>();var engine:ServiceSupervisor?=null
        try{
            val supervisor=ready.get(20,TimeUnit.SECONDS);engine=supervisor
            var previous:String?=null
            repeat(2){index->
                val seen=CountDownLatch(1);val release=CountDownLatch(1);releases.add(release)
                val generation=AtomicReference<String?>()
                val future=supervisor.pull(reference,object:ProbeObserver{
                    override fun onPhase(phase:String){
                        if(JSONObject(phase).optString("phase")=="resolving"&&generation.get()==null){
                            val active=JSONObject(supervisor.packageOperations()).getJSONObject(key)
                            generation.set(active.getString("generation"));seen.countDown()
                            release.await(15,TimeUnit.SECONDS)
                        }
                    }
                })
                futures.add(future)
                // Registration is queued before this actor snapshot. A rejected
                // duplicate is already terminal, so never adopt another user's run.
                val queued=JSONObject(supervisor.packageActivity().get(5,TimeUnit.SECONDS)).getJSONObject("active").optJSONObject(key)
                if(!future.isDone&&queued!=null)owned.add(queued.getString("generation"))
                val deadline=System.nanoTime()+TimeUnit.SECONDS.toNanos(180)
                while(!seen.await(100,TimeUnit.MILLISECONDS)){
                    if(future.isDone)future.get(1,TimeUnit.SECONDS)
                    check(System.nanoTime()<deadline){"Registry resolution did not reach the owned phase"}
                }
                val current=generation.get()!!;owned.add(current)
                if(index==1){assertNotEquals(previous,current);assertFalse(supervisor.cancelPackageOperation(key,previous!!))}
                assertTrue(supervisor.cancelPackageOperation(key,current))
                val live=JSONObject(supervisor.packageActivity().get(5,TimeUnit.SECONDS)).getJSONObject("active").getJSONObject(key)
                assertEquals(current,live.getString("generation"));assertEquals("cancelled",live.getString("status"));assertFalse(future.isDone)
                release.countDown()
                var failed=false
                try{future.get(30,TimeUnit.SECONDS)}catch(expected:ExecutionException){failed=true}
                assertTrue(failed)
                val frame=JSONObject(supervisor.packageActivity().get(5,TimeUnit.SECONDS))
                assertFalse(frame.getJSONObject("active").has(key))
                val receipts=frame.getJSONArray("recent")
                val receipt=(0 until receipts.length()).map{receipts.getJSONObject(it)}.single{it.getString("key")==key}
                assertEquals(current,receipt.getString("generation"));assertEquals("cancelled",receipt.getString("status"))
                previous=current
            }
        }finally{
            releases.forEach{it.countDown()}
            engine?.let{supervisor->owned.forEach{generation->
                supervisor.cancelPackageOperation(key,generation)
            }
                futures.forEach{runCatching{it.get(30,TimeUnit.SECONDS)}}
                owned.forEach{generation->runCatching{supervisor.dismissPackage(key,generation).get(5,TimeUnit.SECONDS)}}
            }
            context.unbindService(connection)
        }
    }
}
