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

/** Explicit live registry/worker acceptance, deferred to the integrated phase.
 * Uses the existing common owner and only uniquely identified test instances. */
@RunWith(AndroidJUnit4::class)
class SupervisedLoopbackTest {
    @Test fun pythonPrivateTcpConnectsToExactSelfListenersWithoutPublishingHostPorts() {
        val context=InstrumentationRegistry.getInstrumentation().targetContext
        val ready=CompletableFuture<ServiceSupervisor>()
        val connection=object:ServiceConnection {
            override fun onServiceConnected(name:ComponentName?,service:IBinder?){ready.complete((service as EngineService.LocalBinder).engine)}
            override fun onServiceDisconnected(name:ComponentName?){}
        }
        assertTrue(context.bindService(Intent(context,EngineService::class.java),connection,Context.BIND_AUTO_CREATE))
        val owned=mutableListOf<String>();var engine:ServiceSupervisor?=null
        try {
            val supervisor=ready.get(20,TimeUnit.SECONDS);engine=supervisor
            val reference="wasmer.io/python/python@3.13.20"
            supervisor.pull(reference).get(300,TimeUnit.SECONDS)
            fun create(script:String,input:Boolean=false,publish:Boolean=false):String {
                val id="test-loopback-${UUID.randomUUID()}"
                val spec=JSONObject().put("artifact_ref",reference).put("command","python")
                    .put("args",JSONArray().put("-u").put("-c").put(script.trimIndent()))
                    .put("stdin_open",input).put("ports",if(publish)JSONArray().put(JSONObject()
                        .put("instance_port",28081).put("host_port",0).put("protocol","tcp"))else JSONArray()).put("network","loopback")
                supervisor.create(id,spec.toString()).get(20,TimeUnit.SECONDS);owned.add(id)
                val started=JSONObject(supervisor.start(id).get(180,TimeUnit.SECONDS))
                assertEquals(if(publish)1 else 0,started.getJSONArray("ports").length())
                return id
            }
            val held=create("""
                import socket, sys
                held = socket.socket()
                held.bind(('0.0.0.0', 28080))
                held.listen(8)
                print('WASMDROID_PRIVATE_HELD', flush=True)
                sys.stdin.readline()
                held.close()
            """,true)
            val until=System.nanoTime()+TimeUnit.SECONDS.toNanos(30)
            var readyOutput=""
            while(System.nanoTime()<until){
                readyOutput=stdout(supervisor.logs(held,"{}").get(10,TimeUnit.SECONDS))
                if(readyOutput.contains("WASMDROID_PRIVATE_HELD"))break
                Thread.sleep(100)
            }
            assertTrue("First service must own a live private listener",readyOutput.contains("WASMDROID_PRIVATE_HELD"))
            val client=create("""
                import socket, threading
                # The other worker's listener must be unreachable, despite using
                # the same virtual IPv4 address and port.
                try:
                    socket.create_connection(('127.0.0.1', 28080), timeout=2)
                    raise AssertionError('another service leaked into this namespace')
                except ConnectionRefusedError:
                    pass
                first = socket.socket()
                first.bind(('0.0.0.0', 28080))
                first.listen(8)
                second = socket.socket()
                second.bind(('127.0.0.1', 0))
                second.listen(8)
                assert second.getsockname()[1] > 0
                scratch = socket.socket()
                scratch.bind(('127.0.0.1', 0))
                unused = scratch.getsockname()[1]
                scratch.close()
                try:
                    socket.create_connection(('127.0.0.1', unused), timeout=2)
                    raise AssertionError('wrong-port connection fell back to another listener')
                except ConnectionRefusedError:
                    pass
                def serve(listener, expected):
                    peer, _ = listener.accept()
                    with peer:
                        data = b''
                        while len(data) < 4:
                            part = peer.recv(4 - len(data))
                            assert part, 'unexpected EOF'
                            data += part
                        assert data == b'ping'
                        peer.sendall(expected)
                threads = [threading.Thread(target=serve, args=(first, b'one!')),
                           threading.Thread(target=serve, args=(second, b'two!'))]
                for thread in threads: thread.start()
                for target, expected in [(first.getsockname(), b'one!'), (second.getsockname(), b'two!')]:
                    with socket.create_connection(('localhost', target[1]), timeout=5) as peer:
                        peer.sendall(b'ping')
                        data = b''
                        while len(data) < 4:
                            part = peer.recv(4 - len(data))
                            assert part, 'unexpected EOF'
                            data += part
                        assert data == expected, (data, expected)
                for thread in threads:
                    thread.join(5)
                    assert not thread.is_alive()
                first.close()
                second.close()
                print('WASMDROID_PRIVATE_LOOPBACK_OK', flush=True)
            """)
            val ended=JSONObject(supervisor.waitForExit(client).get(60,TimeUnit.SECONDS)).getJSONObject("guest_result")
            assertEquals(0,ended.getInt("exit_code"));assertTrue(ended.getString("stdout").contains("WASMDROID_PRIVATE_LOOPBACK_OK"))
            val late=JSONObject(supervisor.waitForExit(client).get(10,TimeUnit.SECONDS)).getJSONObject("guest_result")
            assertEquals(0,late.getInt("exit_code"));assertTrue(late.getString("stdout").contains("WASMDROID_PRIVATE_LOOPBACK_OK"))
            val persistent=create("""
                import socket
                listener = socket.socket()
                listener.bind(('127.0.0.1', 28081))
                listener.listen(8)
                peer, _ = listener.accept()
                with peer:
                    peer.sendall(b'open')
                    data = b''
                    while len(data) < 4:
                        part = peer.recv(4 - len(data))
                        assert part, 'established host stream closed early'
                        data += part
                    assert data == b'late'
                    peer.sendall(b'late')
                listener.close()
                print('WASMDROID_LONG_FORWARD_OK', flush=True)
            """,publish=true)
            val hostPort=JSONArray(supervisor.ports(persistent).get(10,TimeUnit.SECONDS)).getJSONObject(0).getInt("host_port")
            java.net.Socket("127.0.0.1",hostPort).use{socket->
                socket.soTimeout=40_000
                val input=java.io.DataInputStream(socket.getInputStream());val reply=ByteArray(4)
                input.readFully(reply);assertEquals("open",String(reply,Charsets.US_ASCII))
                // Explicit final acceptance for the former blanket 30s timeout.
                Thread.sleep(31_000)
                socket.getOutputStream().write("late".toByteArray(Charsets.US_ASCII))
                input.readFully(reply);assertEquals("late",String(reply,Charsets.US_ASCII))
            }
            val forwarded=JSONObject(supervisor.waitForExit(persistent).get(30,TimeUnit.SECONDS)).getJSONObject("guest_result")
            assertEquals(0,forwarded.getInt("exit_code"))
            assertTrue(forwarded.getString("stdout").contains("WASMDROID_LONG_FORWARD_OK"))
            assertTrue(forwarded.getJSONObject("runtime_info").getBoolean("forwarding_drained"))
            assertEquals("running",JSONObject(supervisor.inspect(held).get(10,TimeUnit.SECONDS)).getString("state"))
            val generation=JSONObject(supervisor.terminalSession(held).get(10,TimeUnit.SECONDS)).getString("generation")
            supervisor.writeStdin(held,generation,byteArrayOf(10)).get(10,TimeUnit.SECONDS)
            val released=JSONObject(supervisor.waitForExit(held).get(30,TimeUnit.SECONDS)).getJSONObject("guest_result")
            assertEquals(0,released.getInt("exit_code"))
        } finally {
            try{engine?.let{supervisor->owned.asReversed().forEach{id->
                supervisor.stop(id,0).get(20,TimeUnit.SECONDS)
                supervisor.remove(id).get(10,TimeUnit.SECONDS)
            }}} finally {context.unbindService(connection)}
        }
    }
    private fun stdout(raw:String?):String {
        if(raw==null)return ""
        val records=JSONObject(raw).getJSONArray("records");val output=java.io.ByteArrayOutputStream()
        for(index in 0 until records.length()){
            val record=records.getJSONObject(index)
            if(record.getString("stream")=="stdout")output.write(Base64.decode(record.getString("data_base64"),Base64.NO_WRAP))
        }
        return output.toString("UTF-8")
    }
}
