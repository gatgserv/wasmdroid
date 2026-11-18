package dev.wasmdroid.app

import android.content.*
import android.os.IBinder
import android.util.Base64
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import dev.wasmdroid.app.engine.EngineService
import dev.wasmdroid.app.engine.ServiceSupervisor
import org.json.JSONObject
import org.junit.Assert.*
import org.junit.Test
import org.junit.runner.RunWith
import java.util.UUID
import java.util.concurrent.*

/** Explicit live registry + real worker/TTY acceptance. Not part of the default
 * hermetic bridge suite. Touches only this test's uniquely named instance. */
@RunWith(AndroidJUnit4::class)
class SupervisedTerminalTest {
    @Test fun realPythonReceivesLongUnicodeInputAndVirtualTerminalSize(){
        val context=InstrumentationRegistry.getInstrumentation().targetContext
        val ready=CompletableFuture<ServiceSupervisor>()
        val connection=object:ServiceConnection{
            override fun onServiceConnected(name:ComponentName?,service:IBinder?){ready.complete((service as EngineService.LocalBinder).engine)}
            override fun onServiceDisconnected(name:ComponentName?){}
        }
        assertTrue(context.bindService(Intent(context,EngineService::class.java),connection,Context.BIND_AUTO_CREATE))
        val id="test-tty-${UUID.randomUUID()}";var engine:ServiceSupervisor?=null;var created=false
        try{
            val supervisor=ready.get(20,TimeUnit.SECONDS);engine=supervisor
            val reference="wasmer.io/python/python@3.13.20"
            supervisor.pull(reference).get(300,TimeUnit.SECONDS)
            val script="""
                import os, sys
                assert sys.stdin.isatty() and sys.stdout.isatty(), 'stdio must be a virtual TTY'
                print('WASMDROID_TTY_READY', flush=True)
                value = sys.stdin.readline()
                assert value == ('notă-' * 1200) + '\n', 'stdin was truncated or corrupted'
                size = os.get_terminal_size(1)
                assert (size.columns, size.lines) == (91, 33), repr(size)
                print('WASMDROID_TTY_OK', flush=True)
            """.trimIndent()
            val spec=JSONObject().put("artifact_ref",reference).put("command","python").put("args",org.json.JSONArray().put("-u").put("-c").put(script))
                .put("tty",true).put("stdin_open",true)
            supervisor.create(id,spec.toString()).get(20,TimeUnit.SECONDS);created=true
            val started=JSONObject(supervisor.start(id).get(180,TimeUnit.SECONDS));val generation=started.getString("generation")
            val capabilities=started.getJSONObject("runtime_info").getJSONObject("driver_capabilities")
            assertEquals("wasmer-local",capabilities.getString("id"))
            assertTrue(capabilities.getBoolean("tty"));assertFalse(capabilities.getBoolean("memory_limit"));assertFalse(capabilities.getBoolean("cpu_shares"))
            val firstStats=JSONObject(supervisor.workerStats(id).get(10,TimeUnit.SECONDS))
            assertEquals(generation,firstStats.getString("generation"));assertEquals("worker_process",firstStats.getString("scope"))
            assertTrue(firstStats.getLong("memory_bytes")>0)
            Thread.sleep(200)
            val nextStats=JSONObject(supervisor.workerStats(id,generation).get(10,TimeUnit.SECONDS))
            assertTrue(nextStats.getLong("sampled_at_monotonic_ns")>firstStats.getLong("sampled_at_monotonic_ns"))
            assertTrue(nextStats.getLong("cpu_time_ns")>=firstStats.getLong("cpu_time_ns"))
            try{supervisor.workerStats(id,"stale-generation").get(5,TimeUnit.SECONDS);fail("Stale statistics must be rejected")}
            catch(expected:ExecutionException){assertTrue(expected.cause?.message?.contains("generation")==true)}
            val deadline=System.nanoTime()+TimeUnit.SECONDS.toNanos(30);var live=""
            while(System.nanoTime()<deadline){live=stdout(supervisor.logs(id,"{}").get(10,TimeUnit.SECONDS));if(live.contains("WASMDROID_TTY_READY"))break;Thread.sleep(100)}
            assertTrue("Ready output must be visible before the guest receives input/exits",live.contains("WASMDROID_TTY_READY"))
            val visible=JSONObject(supervisor.terminalFrame(id,generation,"{}").get(10,TimeUnit.SECONDS))
            assertEquals(25,visible.getInt("rows"));assertEquals(80,visible.getInt("cols"));assertTrue(visible.getBoolean("has_more"))
            val unchanged=JSONObject(supervisor.terminalFrame(id,generation,JSONObject().put("known_version",visible.getLong("version")).toString()).get(10,TimeUnit.SECONDS))
            assertTrue(unchanged.getBoolean("unchanged"))
            try{supervisor.writeStdin(id,"stale-generation",byteArrayOf(10)).get(5,TimeUnit.SECONDS);fail("Stale input must be rejected")}
            catch(expected:ExecutionException){assertTrue(expected.cause?.message?.contains("generation")==true)}
            supervisor.resizeTerminal(id,generation,91,33,910,660).get(10,TimeUnit.SECONDS)
            val input=("notă-".repeat(1200)+"\r").toByteArray(Charsets.UTF_8)
            for(offset in input.indices step 2000)supervisor.writeStdin(id,generation,input.copyOfRange(offset,minOf(offset+2000,input.size))).get(20,TimeUnit.SECONDS)
            val ended=JSONObject(supervisor.waitForExit(id).get(30,TimeUnit.SECONDS)).getJSONObject("guest_result")
            assertEquals(0,ended.getInt("exit_code"));assertTrue(ended.getString("stdout").contains("WASMDROID_TTY_OK"))
            assertTrue(ended.getJSONObject("runtime_info").getBoolean("terminal_snapshot_saved"))
            val finalText=StringBuilder();var row=0
            do{val page=JSONObject(supervisor.savedTerminal(id,generation,row.toUShort()).get(10,TimeUnit.SECONDS)?:error("Final terminal snapshot missing"))
                assertEquals(33,page.getInt("rows"));assertEquals(91,page.getInt("cols"));val lines=page.getJSONArray("cells")
                for(index in 0 until lines.length()){val line=lines.getJSONArray(index);for(col in 0 until line.length())finalText.append(line.getJSONArray(col).getString(0));finalText.append('\n')};row+=lines.length()
            }while(page.getBoolean("has_more"))
            assertTrue(finalText.toString().contains("WASMDROID_TTY_OK"))
            assertTrue(stdout(supervisor.logs(id,"{}").get(10,TimeUnit.SECONDS)).contains("WASMDROID_TTY_OK"))
        }finally{
            try{if(created)engine?.let{supervisor->supervisor.stop(id,0).get(20,TimeUnit.SECONDS);supervisor.remove(id).get(10,TimeUnit.SECONDS)}}
            finally{context.unbindService(connection)}
        }
    }
    private fun stdout(raw:String?):String{
        if(raw==null)return ""
        val records=JSONObject(raw).getJSONArray("records");val bytes=java.io.ByteArrayOutputStream()
        for(index in 0 until records.length()){val record=records.getJSONObject(index);if(record.getString("stream")=="stdout")bytes.write(Base64.decode(record.getString("data_base64"),Base64.NO_WRAP))}
        return bytes.toString("UTF-8")
    }
}
