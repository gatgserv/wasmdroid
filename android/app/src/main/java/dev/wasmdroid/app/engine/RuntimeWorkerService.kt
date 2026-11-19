package dev.wasmdroid.app.engine

import android.app.Service
import android.content.Intent
import android.os.*
import dev.wasmdroid.engine.ServiceRuntime
import dev.wasmdroid.engine.WorkerControl
import dev.wasmdroid.engine.WorkerExecutionIdentity
import dev.wasmdroid.engine.WorkerEgressReplacement
import java.io.File
import java.util.concurrent.atomic.AtomicBoolean
import java.util.concurrent.atomic.AtomicInteger
import java.util.concurrent.atomic.AtomicLong
import org.json.JSONObject

/** One service's execution boundary. Registry/state ownership stays in the
 * coordinator. Native guest work never runs on the Binder/control main thread. */
open class RuntimeWorkerService : Service() {
    private val main = Handler(Looper.getMainLooper())
    private val stopping = AtomicBoolean(false)
    private val activeControls = AtomicInteger(0)
    private val stopGraceSeconds = AtomicLong(5)
    private val workerControl = object : WorkerControl {
        override fun terminateAfter(graceSeconds: ULong) {
            val delayMillis = graceSeconds.coerceAtMost(60uL).toLong() * 1000
            main.postDelayed({ terminateSelf() }, delayMillis)
        }
        override fun terminate() { terminateSelf() }
    }
    private var generation: String? = null
    private var instanceId:String?=null
    private val lan by lazy{WorkerLanHost(this,main,
        {val id=instanceId;val token=generation;if(id!=null&&token!=null)id to token else null},
        {!stopping.get()&&generation!=null},{runtime})}
    private var coordinator: Messenger? = null
    private var coordinatorDeath: IBinder.DeathRecipient? = null
    @Volatile private var runtime: ServiceRuntime? = null
    @Volatile private var egressHost:WorkerEgressHost?=null
    @Volatile private var regrant:EgressRegrantCoordinator<WorkerEgressHost.Capture,WorkerEgressReplacement>?=null
    private val control = Messenger(Handler(Looper.getMainLooper()) { message ->
        if (message.sendingUid != Process.myUid()) return@Handler true
        val data = message.data
        val token = data.getString(WorkerProtocol.GENERATION)
        if (message.what == WorkerProtocol.START) {
            @Suppress("DEPRECATION")
            val owner = data.getParcelable<ParcelFileDescriptor>(WorkerProtocol.OWNER)
            if (generation != null || token.isNullOrEmpty() || owner == null || message.replyTo == null) {
                owner?.close()
                return@Handler true
            }
            generation = token
            instanceId=data.getString(WorkerProtocol.INSTANCE)
            coordinator = message.replyTo
            val death = IBinder.DeathRecipient { terminateSelf() }
            coordinatorDeath = death
            try { message.replyTo.binder.linkToDeath(death, 0) }
            catch (_: RemoteException) { owner.close(); terminateSelf(); return@Handler true }
            if (data.getBoolean(WorkerProtocol.STOP_BEFORE_START)) {
                owner.close(); terminateSelf(); return@Handler true
            }
            val spec = data.getString(WorkerProtocol.SPEC) ?: ""
            val id = data.getString(WorkerProtocol.INSTANCE) ?: ""
            val network=data.getString(WorkerProtocol.NETWORK_CONFIG)
            val egressRequired=data.getBoolean(WorkerProtocol.EGRESS_REQUIRED)
            Thread({ execute(id,token,spec,owner,network,egressRequired) }, "wasmdroid-service-run").start()
        } else if (token == generation && generation != null) {
            when (message.what) {
                WorkerProtocol.STOP -> {
                    stopGraceSeconds.set(data.getLong(WorkerProtocol.GRACE_SECONDS, 5).coerceIn(0,60))
                    stopping.set(true)
                    regrant?.stop()
                    egressHost?.revokeAll()
                    lan.revokeAll()
                    // A native stop call can hang; the control Looper stays
                    // responsive to the coordinator's final terminate message.
                    Thread({ runCatching { runtime?.requestStop(stopGraceSeconds.get().toULong()) } }, "wasmdroid-service-stop").start()
                }
                WorkerProtocol.TERMINATE, WorkerProtocol.COMPLETE_ACK -> {
                    // Driver kill uses only the trusted self-termination callback,
                    // outside native execution locks. Startup has no facade yet.
                    runCatching { runtime?.kill() }
                    terminateSelf()
                }
                WorkerProtocol.PING -> send(WorkerProtocol.PONG, "{}")
                WorkerProtocol.LAN_REVOKE -> runCatching{lan.revoke(data)}
                WorkerProtocol.STDIN,WorkerProtocol.STDIN_EOF,WorkerProtocol.RESIZE,WorkerProtocol.INTERRUPT,WorkerProtocol.TERMINAL_FRAME,WorkerProtocol.STATS,WorkerProtocol.NETWORK,WorkerProtocol.LAN_ENABLE,WorkerProtocol.LAN_DISABLE,WorkerProtocol.LAN_STATUS -> {
                    val request=data.getString(WorkerProtocol.REQUEST)?:return@Handler true
                    val what=message.what
                    val bytes=data.getByteArray(WorkerProtocol.BYTES)?:ByteArray(0)
                    if(bytes.size>4096||activeControls.incrementAndGet()>4){
                        if(bytes.size<=4096)activeControls.decrementAndGet()
                        if(what==WorkerProtocol.LAN_ENABLE&&bytes.isEmpty()){
                            try{send(WorkerProtocol.CONTROL_DONE,lan.declineEnable(data),request)}
                            catch(_:Exception){send(WorkerProtocol.CONTROL_ERROR,"LAN admission could not be declined",request)}
                        }else send(WorkerProtocol.CONTROL_ERROR,"Terminal control is busy",request)
                        return@Handler true
                    }
                    // Messenger FIFO makes this reservation visible to later
                    // Revoke/Stop messages before a native thread can enter.
                    // Preparation performs no native call or network bind.
                    val lanReservation=try{
                        if(what==WorkerProtocol.LAN_ENABLE)lan.reserveEnable(data)else null
                    }catch(failure:Exception){
                        activeControls.decrementAndGet()
                        send(WorkerProtocol.CONTROL_ERROR,failure.message?:"LAN preparation failed",request)
                        return@Handler true
                    }
                    Thread({
                        try{
                            val result=when(what){
                                WorkerProtocol.LAN_ENABLE->lan.enable(requireNotNull(lanReservation))
                                WorkerProtocol.LAN_DISABLE->lan.disable(data)
                                WorkerProtocol.LAN_STATUS->lan.status(data)
                                else->{
                                    val execution=runtime?:error("Runtime is not ready")
                                    when(what){
                                        WorkerProtocol.TERMINAL_FRAME->execution.terminalFrame(data.getString(WorkerProtocol.QUERY)?:"{}")
                                        WorkerProtocol.STATS->execution.stats()
                                        WorkerProtocol.NETWORK->{execution.updateNetwork(data.getString(WorkerProtocol.NETWORK_CONFIG)?:error("Missing network configuration"));"{}"}
                                        else->{
                                            when(what){
                                                WorkerProtocol.STDIN->execution.writeStdin(bytes)
                                                WorkerProtocol.STDIN_EOF->execution.closeStdin()
                                                WorkerProtocol.INTERRUPT->execution.interrupt()
                                                WorkerProtocol.RESIZE->execution.resizeTerminal(data.getInt(WorkerProtocol.COLS).toUInt(),data.getInt(WorkerProtocol.ROWS).toUInt(),data.getInt(WorkerProtocol.WIDTH).toUInt(),data.getInt(WorkerProtocol.HEIGHT).toUInt())
                                            };"{}"
                                        }
                                    }
                                }
                            }
                            send(WorkerProtocol.CONTROL_DONE,result,request)
                        }catch(failure:Exception){send(WorkerProtocol.CONTROL_ERROR,failure.message?:"Terminal operation failed",request)}
                        finally{activeControls.decrementAndGet()}
                    },"wasmdroid-terminal-control").start()
                }
            }
        }
        true
    })

    override fun onBind(intent: Intent?): IBinder {lan.start();return control.binder}

    private fun execute(id:String,token:String,spec: String, owner: ParcelFileDescriptor,network:String?,egressRequired:Boolean) {
        try {
            require(spec.toByteArray(Charsets.UTF_8).size <= 128 * 1024) { "Service specification too large" }
            require(network==null||network.toByteArray(Charsets.UTF_8).size<=32*1024){"Network configuration exceeds limit"}
            val parsed=JSONObject(spec)
            val outbound=listOf("internet_access","lan_access").any{field->
                if(!parsed.has(field))false else parsed.get(field).let{require(it is Boolean);it}
            }
            val capture=if(outbound||egressRequired){
                WorkerEgressHost(this,main){!stopping.get()&&generation==token&&instanceId==id}
                    .also{egressHost=it}.capture()
            }else null
            val descriptor = owner.fd
            // Rust duplicates this borrowed fd after actual native entry. The
            // original remains owned even if generated argument lowering fails.
            val execution = if(capture==null)ServiceRuntime.startBorrowed(File(filesDir, "engine").absolutePath, descriptor,id,token,spec,workerControl,network)
                else ServiceRuntime.startWithEgress(File(filesDir,"engine").absolutePath,descriptor,WorkerExecutionIdentity(id,token),spec,workerControl,network,capture)
            runtime = execution
            if(capture!=null){
                val host=checkNotNull(egressHost)
                regrant=EgressRegrantCoordinator(object:EgressRegrantBackend<WorkerEgressHost.Capture,WorkerEgressReplacement>{
                    override fun attachment()=EgressRegrantCodec.attachment(execution.egressAttachmentStatus())
                    override fun acquire()=host.requestCapture()
                    override fun epoch(capture:WorkerEgressHost.Capture)=capture.networkEpoch
                    override fun release(capture:WorkerEgressHost.Capture)=host.release(capture)
                    override fun begin(capture:WorkerEgressHost.Capture)=execution.beginEgressReplacement(capture)
                    override fun dispatch(operation:WorkerEgressReplacement)=operation.dispatch()
                    override fun status(operation:WorkerEgressReplacement)=EgressRegrantCodec.replacement(operation.status())
                    override fun cancel(operation:WorkerEgressReplacement)=operation.cancel()
                },capture)
                if(stopping.get())regrant?.stop()
            }
            if (stopping.get()) execution.requestStop(stopGraceSeconds.get().toULong())
            send(WorkerProtocol.READY, JSONObject(execution.info())
                .put("driver_capabilities",JSONObject(execution.capabilities())).toString())
            var nextEgressTick=0L
            while (true) {
                val result = execution.poll()
                if (result != null) {
                    val stopReceived = stopping.getAndSet(true)
                    regrant?.stop();egressHost?.revokeAll();lan.revokeAll()
                    val report=JSONObject(result).put("runtime_info",JSONObject(execution.info()))
                        .put("worker_stats",JSONObject(execution.stats()))
                    send(WorkerProtocol.COMPLETED, report.toString(), stopReceived = stopReceived)
                    // Let the coordinator capture the exit before Binder death.
                    main.postDelayed({ terminateSelf() }, 2_000)
                    return
                }
                val now=SystemClock.elapsedRealtime()
                if(now>=nextEgressTick){regrant?.tick(now);nextEgressTick=now+250}
                Thread.sleep(50)
            }
        } catch (_: InterruptedException) {
            send(WorkerProtocol.FAILED, "Service execution was interrupted")
            main.postDelayed({ terminateSelf() }, 2_000)
        } catch (failure: Exception) {
            // Private user-visible IPC only. Never send args/env or host paths
            // to analytics/logcat. Rust supplies scoped operation errors.
            send(WorkerProtocol.FAILED, failure.message ?: "Service execution failed")
            main.postDelayed({ terminateSelf() }, 2_000)
        } finally { regrant?.stop();egressHost?.close();owner.close() }
    }

    private fun send(what: Int, result: String, request:String?=null, stopReceived:Boolean=false) {
        val message = Message.obtain(null, what).apply {
            data = Bundle().apply {
                putString(WorkerProtocol.GENERATION, generation)
                putString(WorkerProtocol.RESULT, result)
                if (stopReceived) putBoolean(WorkerProtocol.STOP_RECEIVED, true)
                request?.let{putString(WorkerProtocol.REQUEST,it)}
                putInt(WorkerProtocol.PID, Process.myPid())
                putInt(WorkerProtocol.UID, Process.myUid())
            }
        }
        try { coordinator?.send(message) } catch (_: RemoteException) { terminateSelf() }
    }

    override fun onUnbind(intent: Intent?): Boolean { terminateSelf(); return false }
    override fun onDestroy() {
        regrant?.stop();egressHost?.close()
        lan.close()
        coordinatorDeath?.let { runCatching { coordinator?.binder?.unlinkToDeath(it, 0) } }
        terminateSelf()
        super.onDestroy()
    }
    private fun terminateSelf() { Process.killProcess(Process.myPid()) }
}

// API 28 supports private manifest-named processes. This is a bounded worker
// pool, not separate engine instances. Slots are released only after Binder death.
class RuntimeSlot0 : RuntimeWorkerService()
class RuntimeSlot1 : RuntimeWorkerService()
class RuntimeSlot2 : RuntimeWorkerService()
class RuntimeSlot3 : RuntimeWorkerService()
class RuntimeSlot4 : RuntimeWorkerService()
class RuntimeSlot5 : RuntimeWorkerService()
class RuntimeSlot6 : RuntimeWorkerService()
class RuntimeSlot7 : RuntimeWorkerService()
