package dev.wasmdroid.app

import android.content.*
import android.os.IBinder
import android.util.Base64
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

/** Explicit live arm64 acceptance definition. Real public Python TCP services
 * exercise private IPC; this is not a curated database/application recipe. */
@RunWith(AndroidJUnit4::class)
class SupervisedStackTest {
    @Test fun privateRequestReplySelectiveRestartAndPersistentDataKeepTheClientGeneration() {
        val context=InstrumentationRegistry.getInstrumentation().targetContext
        val ready=CompletableFuture<ServiceSupervisor>()
        val connection=object:ServiceConnection {
            override fun onServiceConnected(name:ComponentName?,service:IBinder?){ready.complete((service as EngineService.LocalBinder).engine)}
            override fun onServiceDisconnected(name:ComponentName?){}
        }
        assertTrue(context.bindService(Intent(context,EngineService::class.java),connection,Context.BIND_AUTO_CREATE))
        val id="test-stack-${UUID.randomUUID()}";var engine:ServiceSupervisor?=null;var created=false
        val volumes=mutableListOf<String>()
        try {
            val supervisor=ready.get(20,TimeUnit.SECONDS);engine=supervisor
            supervisor.pull("wasmer.io/python/python@3.13.20").get(300,TimeUnit.SECONDS)
            val database="""
                import socket, pathlib
                path = pathlib.Path('/data/count')
                count = int(path.read_text()) if path.exists() else 0
                listener = socket.socket()
                listener.bind(('0.0.0.0', 5432))
                listener.listen(8)
                print('STACK_DATABASE_READY', flush=True)
                while True:
                    try:
                        peer, _ = listener.accept()
                        with peer:
                            data = b''
                            while len(data) < 4:
                                part = peer.recv(4 - len(data))
                                if not part: break
                                data += part
                            if data != b'ping': continue
                            count += 1
                            path.write_text(str(count))
                            peer.sendall((str(count) + '\n').encode())
                    except OSError:
                        break
            """.trimIndent()
            val client="""
                import socket, sys, time
                print('STACK_CLIENT_READY', flush=True)
                for command in sys.stdin:
                    if command.strip() != 'probe': continue
                    until = time.monotonic() + 10
                    while True:
                        try:
                            with socket.create_connection(('database', 5432), timeout=3) as peer:
                                peer.sendall(b'ping')
                                value = b''
                                while not value.endswith(b'\n'):
                                    part = peer.recv(16)
                                    assert part, 'unexpected private stream EOF'
                                    value += part
                                print('STACK_REPLY:' + value.decode().strip(), flush=True)
                            break
                        except OSError:
                            if time.monotonic() >= until: raise
                            time.sleep(0.1)
            """.trimIndent()
            fun service(script:String)=JSONObject().put("image","wasmer.io/python/python@3.13.20")
                .put("command",JSONArray().put("python").put("-u").put("-c").put(script))
            val document=JSONObject().put("services",JSONObject()
                .put("database",service(database).put("volumes",JSONArray().put("data:/data")))
                .put("client",service(client).put("stdin_open",true).put("depends_on",JSONArray().put("database"))))
                .put("volumes",JSONObject().put("data",JSONObject())).toString()
            val definition=JSONObject(supervisor.createStack(id,document).get(20,TimeUnit.SECONDS));created=true
            val declared=definition.getJSONObject("plan").getJSONObject("volumes")
            declared.keys().forEach{name->volumes.add(declared.getJSONObject(name).getString("name"))}
            supervisor.startStack(id).get(360,TimeUnit.SECONDS)
            val members=JSONArray(supervisor.stackMembers(id).get(10,TimeUnit.SECONDS))
            fun member(name:String)=(0 until members.length()).map{members.getJSONObject(it)}.first{it.getString("service")==name}.getString("instance_id")
            val databaseId=member("database");val clientId=member("client")
            val clientGeneration=JSONObject(supervisor.terminalSession(clientId).get(10,TimeUnit.SECONDS)).getString("generation")
            val oldDatabaseGeneration=JSONObject(supervisor.terminalSession(databaseId).get(10,TimeUnit.SECONDS)).getString("generation")
            assertEquals(0,JSONArray(supervisor.ports(databaseId).get(10,TimeUnit.SECONDS)).length())
            assertEquals(0,JSONArray(supervisor.ports(clientId).get(10,TimeUnit.SECONDS)).length())
            awaitOutput(supervisor,clientId,"STACK_CLIENT_READY")
            supervisor.writeStdin(clientId,clientGeneration,"probe\n".toByteArray()).get(10,TimeUnit.SECONDS)
            awaitOutput(supervisor,clientId,"STACK_REPLY:1")
            supervisor.stop(databaseId,0).get(20,TimeUnit.SECONDS)
            assertEquals(clientGeneration,JSONObject(supervisor.terminalSession(clientId).get(10,TimeUnit.SECONDS)).getString("generation"))
            supervisor.start(databaseId).get(180,TimeUnit.SECONDS)
            assertNotEquals(oldDatabaseGeneration,JSONObject(supervisor.terminalSession(databaseId).get(10,TimeUnit.SECONDS)).getString("generation"))
            assertEquals(clientGeneration,JSONObject(supervisor.terminalSession(clientId).get(10,TimeUnit.SECONDS)).getString("generation"))
            supervisor.writeStdin(clientId,clientGeneration,"probe\n".toByteArray()).get(10,TimeUnit.SECONDS)
            awaitOutput(supervisor,clientId,"STACK_REPLY:2")
            supervisor.stopStack(id).get(40,TimeUnit.SECONDS)
            val stopped=JSONArray(supervisor.stackMembers(id).get(10,TimeUnit.SECONDS))
            for(index in 0 until stopped.length())assertTrue(stopped.getJSONObject(index).getString("state") in listOf("exited","dead"))
        } finally {
            try{engine?.let{supervisor->if(created){
                supervisor.stopStack(id).get(40,TimeUnit.SECONDS)
                val version=JSONObject(supervisor.inspectStack(id).get(10,TimeUnit.SECONDS)).getLong("version").toULong()
                supervisor.removeStack(id,version).get(20,TimeUnit.SECONDS)
                for(volume in volumes)supervisor.removeVolume(volume).get(10,TimeUnit.SECONDS)
            }}} finally {context.unbindService(connection)}
        }
    }
    private fun awaitOutput(engine:ServiceSupervisor,id:String,marker:String){
        val deadline=System.nanoTime()+TimeUnit.SECONDS.toNanos(30)
        while(System.nanoTime()<deadline){
            val raw=engine.logs(id,"{}").get(10,TimeUnit.SECONDS)
            if(raw!=null){val bytes=java.io.ByteArrayOutputStream();val records=JSONObject(raw).getJSONArray("records")
                for(index in 0 until records.length()){val record=records.getJSONObject(index);if(record.getString("stream")=="stdout")bytes.write(Base64.decode(record.getString("data_base64"),Base64.NO_WRAP))}
                if(bytes.toString("UTF-8").contains(marker))return
            };Thread.sleep(100)
        };fail("Expected private service output was not observed: $marker")
    }
}
