package dev.wasmdroid.app.engine

import android.content.*
import android.os.*
import dev.wasmdroid.app.LocalNetworkPermission
import dev.wasmdroid.engine.EngineStorage
import dev.wasmdroid.engine.GroupNetworkPermissionReview
import dev.wasmdroid.engine.ProbeObserver
import dev.wasmdroid.engine.DataTransferControl
import dev.wasmdroid.engine.DataReadTransfer
import dev.wasmdroid.engine.DataWriteTransfer
import dev.wasmdroid.engine.BackupRestoreTransfer
import dev.wasmdroid.engine.InstallControl
import org.json.JSONArray
import org.json.JSONObject
import java.io.File
import java.util.UUID
import java.util.concurrent.CompletableFuture
import java.util.concurrent.Executors
import java.util.concurrent.ConcurrentHashMap

internal const val CONFIRMED_OWNER_STOP = "Stopped by owner after verified guest exit"

internal fun workerExitReason(
    exitCode: Int?,
    workerError: String?,
    ownerStopRequested: Boolean,
    workerReceivedStop: Boolean,
): String? = workerError ?: when {
    exitCode == null -> "Worker exited without a verified guest result"
    exitCode != 0 && ownerStopRequested && workerReceivedStop -> CONFIRMED_OWNER_STOP
    else -> null
}

/** Common owner: durable state/registry/volumes plus one controller per service.
 * All controller mutations run on this actor; no blocking operation runs on UI. */
class ServiceSupervisor(context: Context, private val onActivity: (Activity) -> Unit = {}) {
    /** Work ownership and human-facing application counts have different units.
     * Only ownedCount participates in the existing lifetime handshake. */
    data class Activity(val ownedCount: Int, val summary: NotificationSummary.Snapshot)
    private val context = context.applicationContext
    private val thread = HandlerThread("wasmdroid-supervisor").apply { start() }
    private val actor = Handler(thread.looper)
    private val io = Executors.newFixedThreadPool(2)
    private val logIo = Executors.newSingleThreadExecutor()
    private val storage = CompletableFuture<EngineStorage>()
    private val storageUsageLock = Any()
    private var storageUsageWork: CompletableFuture<String>? = null
    private var closing = false
    private var storageClosed = false
    private val lifetime=ServiceLifetime()
    private val bootEpoch=AndroidBootEpoch.read(this.context)
    private var recoveryResult:CompletableFuture<String>?=null
    private class RecoveryScan(val answer:CompletableFuture<String>){var cursor:String?=null;var scheduled=0;var skipped=0;var scanned=0L}
    private var recoveryScan:RecoveryScan?=null
    private val browserSessions=BrowserSessionAuthority()
    private val catalogUpdates=CatalogUpdateCoordinator(this.context,storage,io,{action->actor.post(action)},{!closing},{finishCloseIfIdle()})
    private val jobs = linkedMapOf<String, Job>()
    private val lanSharing=LanSharingCoordinator(this.context,actor,{!closing&&lifetime.admits()},
        {endpoint->jobs[endpoint.id]?.let{it.generation==endpoint.generation&&it.ready&&!it.stopping}==true},
        {endpoint,what,extras,admission->terminalCommand(endpoint.id,endpoint.generation,what,extras,admission)},
        {endpoint,extras->jobs[endpoint.id]?.takeIf{it.generation==endpoint.generation}?.let{job->runCatching{send(job,WorkerProtocol.LAN_REVOKE,extras)}};Unit})
    private val stackStarts=mutableMapOf<String,StackStart>()
    private val stackEdits=mutableMapOf<String,String>()
    private var networkRevision=0uL
    private val automaticFailures=linkedMapOf<String,String>()
    private val pendingBoot=linkedMapOf<String,BootApplicationPlan.Candidate>()
    private class BootRun(val candidates:List<BootApplicationPlan.Candidate>,val result:CompletableFuture<String>){
        var position=0;var started=0;var failed=0;var skipped=0;var active:Pair<String,String>?=null
        val cancelled=mutableSetOf<String>()
    }
    private var bootRun:BootRun?=null
    private var bootResult:CompletableFuture<String>?=null
    private var activeCountPosted=false
    private val restarts=ServiceRestartCoordinator(
        {delay,action->check(actor.postDelayed({action()},delay)){"Supervisor is closed"}},
        {id->restartRecord(id)},
        {ticket->launchRestart(ticket)},
        {notifyActiveCount()},
        {ticket,_->recordAutomaticFailure(ticket.id,"restart_admission_failed")},
    )
    private class StackStart(val id:String,val members:List<Job>,val added:List<Job>,val order:List<Job>,val result:CompletableFuture<String>){var cancelled=false;var rollingBack=false;var retryingFence=false}
    // Bounded recent completion receipts make fast exits race-independent.
    // A new generation invalidates its old receipt before becoming visible.
    private val recentExits = linkedMapOf<String,String>()
    private val dataOwners=mutableMapOf<String,String>()
    private val packageOperations=ConcurrentHashMap<String,PackageOperation>()
    private val recentPackages=linkedMapOf<String,PackageReceipt>()
    private val localPackages=LocalPackageImports(this.context,storage,io,{action->actor.post(action)},
        {operation->
            check(!closing&&packageOperations.size<16){"Supervisor is closing or too many package operations"}
            packageOperations[operation.key]=operation;recentPackages.remove(operation.key);notifyActiveCount()
        },{operation,status,errorKind->
            val receipt=operation.snapshot(status).put("image_committed",status=="installed")
            errorKind?.let{receipt.put("error_kind",it).put("technical_error","Local package operation failed ($it)")}
            recentPackages[operation.key]=PackageReceipt(receipt.toString(),operation.retry)
            while(recentPackages.size>16)recentPackages.remove(recentPackages.keys.first())
            packageOperations.remove(operation.key,operation);operation.control.close();finishCloseIfIdle()
        })
    private val slots = listOf(RuntimeSlot0::class.java, RuntimeSlot1::class.java,
        RuntimeSlot2::class.java, RuntimeSlot3::class.java, RuntimeSlot4::class.java,
        RuntimeSlot5::class.java, RuntimeSlot6::class.java, RuntimeSlot7::class.java)
    private val closeFuture = CompletableFuture<Unit>()

    private class Job(val id: String, val slot: Int, var version: ULong, val spec: String,val stackId:String?=null) {
        val generation: String = UUID.randomUUID().toString()
        val started = CompletableFuture<String>()
        val ended = CompletableFuture<String>()
        var messenger: Messenger? = null
        var binder: IBinder? = null
        var connection: ServiceConnection? = null
        var stopping = false
        var stopRequested = false
        var forced = false
        var bound = false
        var exit: Int? = null
        var result: String? = null
        var error: String? = null
        var receivedStopBeforeExit = false
        var ports = JSONArray()
        var runtimeInfo = JSONObject()
        var ready = false
        var readyAtMillis:Long?=null
        var authority:JSONObject?=null
        var egressRequired=false
        var managedAdmissionAttempted=false
        val commands=mutableMapOf<String,CompletableFuture<String>>()
        var stdinCommand:String?=null
        var networkConfig:String?=null
        var networkVersion=0uL
        var bindRequested=false
    }

    init {
        actor.post {
            try { storage.complete(EngineStorage.open(File(this.context.filesDir, "engine").absolutePath)) }
            catch (failure: Exception) { storage.completeExceptionally(failure) }
        }
    }
    private fun <T> submit(block: (EngineStorage) -> T): CompletableFuture<T> {
        val future = CompletableFuture<T>()
        val posted=actor.post {
            try { check(!closing&&lifetime.admits()) { "Supervisor is closing or retiring" }; future.complete(block(storage.join())) }
            catch (failure: Exception) { future.completeExceptionally(failure) }
        }
        if(!posted)future.completeExceptionally(IllegalStateException("Supervisor is closed"))
        return future
    }
    private fun <T> disk(block: (EngineStorage) -> T): CompletableFuture<T> {
        val answer=CompletableFuture<T>()
        try {
            lifetime.beginBackground()
        } catch (failure: Exception) {
            answer.completeExceptionally(failure)
            return answer
        }
        actor.post{notifyActiveCount()}
        try{
            // The actual IO future is private. Caller cancellation/completion
            // cannot suppress its ownership-release callback.
            CompletableFuture.supplyAsync({block(storage.join())},io).whenComplete{value,failure->
                lifetime.endBackground();actor.post{notifyActiveCount();finishCloseIfIdle()}
                if(failure==null)answer.complete(value)else answer.completeExceptionally(failure)
            }
        }catch(failure:Exception){
            lifetime.endBackground();actor.post{notifyActiveCount();finishCloseIfIdle()}
            answer.completeExceptionally(failure)
        }
        return answer
    }

    private fun ownedCount():Int = jobs.size+restarts.pendingCount+pendingBoot.size
    private fun notifyActiveCount(){
        lifetime.owned(ownedCount())
        // Coalesce within the actor turn. Cancel+reserve is one admission;
        // observers must never see an intermediate zero and stop the FGS.
        if(!activeCountPosted){
            activeCountPosted=true
            actor.post{activeCountPosted=false;onActivity(Activity(ownedCount(),notificationSnapshot()))}
        }
    }
    private fun notificationSnapshot():NotificationSummary.Snapshot {
        val observations=mutableListOf<NotificationSummary.ApplicationWork>()
        jobs.values.forEach{job->
            val key=job.stackId?.let{NotificationSummary.ApplicationKey(NotificationSummary.Kind.Group,it)}
                ?:NotificationSummary.ApplicationKey(NotificationSummary.Kind.Standalone,job.id)
            val phase=when{job.stopping->NotificationSummary.Phase.Stopping;!job.ready->NotificationSummary.Phase.Starting;else->NotificationSummary.Phase.Running}
            observations.add(NotificationSummary.ApplicationWork(key,phase))
        }
        // Both native boot admission and restart tickets exclude managed group
        // members. Their reservations are standalone apps, not extra workers.
        val pending=(restarts.pendingTickets().map{it.id}+pendingBoot.values.map{it.id}).distinct()
        pending.forEach{id->observations.add(NotificationSummary.ApplicationWork(
            NotificationSummary.ApplicationKey(NotificationSummary.Kind.Standalone,id),NotificationSummary.Phase.Starting))}
        val background=lifetime.backgroundCount()>0||dataOwners.isNotEmpty()||stackEdits.isNotEmpty()||
            packageOperations.isNotEmpty()||!catalogUpdates.idle||bootRun!=null||recoveryScan!=null
        return NotificationSummary.summarize(observations,background)
    }
    fun notificationSummary():CompletableFuture<NotificationSummary.Snapshot> = submit{notificationSnapshot()}
    fun activeCount():CompletableFuture<Int>{
        val answer=CompletableFuture<Int>()
        if(!actor.post{answer.complete(ownedCount())})answer.completeExceptionally(IllegalStateException("Supervisor is closed"))
        return answer
    }
    fun ownerBound(){lifetime.bound()}
    fun ownerUnbound(){lifetime.unbound()}
    fun ownerStarted(){lifetime.started()}
    fun requestIdleRetirement():CompletableFuture<ServiceLifetime.IdlePermit?>{
        val answer=CompletableFuture<ServiceLifetime.IdlePermit?>()
        if(!actor.post{
            val idle=!closing&&jobs.isEmpty()&&restarts.pendingCount==0&&pendingBoot.isEmpty()&&bootRun==null&&recoveryScan==null&&
                dataOwners.isEmpty()&&stackStarts.isEmpty()&&stackEdits.isEmpty()&&packageOperations.isEmpty()&&catalogUpdates.idle
            answer.complete(lifetime.reserveIdle(idle))
        })answer.complete(null)
        return answer
    }
    fun commitIdleRetirement(permit:ServiceLifetime.IdlePermit,action:(Boolean)->Unit):Boolean=lifetime.commit(permit,action)
    /** Backend status for future settings/status UI; contains no arguments,
     * environment, mounted paths or exception text. */
    fun automationStatus():CompletableFuture<String> = submit{
        JSONObject().put("restarting",JSONArray(restarts.pendingTickets().map{ticket->
            JSONObject().put("id",ticket.id).put("source_id",ticket.sourceId).put("generation",ticket.generation)
        })).put("boot_pending",JSONArray(pendingBoot.values.map{it.id}))
            .put("failures",JSONObject(automaticFailures as Map<*,*>))
            .put("boot_identity_available",bootEpoch!=null)
            .put("recovery",runCatching{recoveryResult?.getNow(null)?.let(::JSONObject)}.getOrNull()?:JSONObject.NULL).toString()
    }
    fun noteRecoveryUnavailable(kind:String):CompletableFuture<Unit> = submit{
        require(kind=="foreground_start_denied"){"Unknown recovery admission failure"}
        recordAutomaticFailure("owner",kind)
    }
    fun browserEndpoint(id:String):CompletableFuture<String> = submit{
        jobs[id]?.takeIf{it.ready&&!it.stopping}?.let{job->
            JSONObject().put("id",job.id).put("generation",job.generation).put("ports",job.ports).toString()
        }?:"null"
    }
    fun shortcutTargets(after:String?,limit:Int):CompletableFuture<String>{
        require(limit in 1..50)
        return disk{it.applicationTargets(after,limit.toUInt())}.thenCompose{raw->submit{db->
            val page=JSONObject(raw);val records=page.getJSONArray("targets")
            val targets=JSONArray()
            for(index in 0 until records.length()){
                // Re-resolve native lineage after IO before adding live state.
                val target=records.getJSONObject(index).getJSONObject("target").toString()
                runCatching{applicationDescriptor(db,JSONObject(db.resolveApplicationTarget(target)))}.getOrNull()?.let(targets::put)
            }
            page.put("targets",targets).toString()
        }}
    }
    fun shortcutResolve(targetJson:String):CompletableFuture<String> = submit{db->
        applicationDescriptor(db,JSONObject(db.resolveApplicationTarget(targetJson))).toString()
    }
    fun applicationTarget(kind:String,id:String):CompletableFuture<String> = submit{db->
        applicationDescriptor(db,JSONObject(db.applicationTarget(kind,id))).toString()
    }
    private fun applicationDescriptor(db:EngineStorage,record:JSONObject):JSONObject{
        val target=record.getJSONObject("target");val members=record.getJSONArray("members")
        val ids=(0 until members.length()).map{members.getJSONObject(it).getString("id")}
        val group=target.getString("kind")=="group"
        val live=ids.mapNotNull{jobs[it]}
        val restartPending=restarts.pendingTickets().filter{it.id in ids||it.sourceId in ids}
        val bootPending=pendingBoot.values.filter{it.id in ids}
        val automaticPending=restartPending.isNotEmpty()||bootPending.isNotEmpty()
        val changing=ids.any{it in dataOwners}||(group&&target.getString("id") in stackEdits)||closing||!lifetime.admits()
        val stopping=live.any{it.stopping}
        val starting=live.any{!it.ready&&!it.stopping}||automaticPending||(group&&target.getString("id") in stackStarts)
        val running=ids.isNotEmpty()&&ids.all{jobs[it]?.let{job->job.ready&&!job.stopping}==true}
        val idle=(0 until members.length()).all{members.getJSONObject(it).getString("state") in setOf("created","exited","dead")}
        val state=when{changing||ids.isEmpty()->"unavailable";stopping->"stopping";starting->"starting";running->"running";live.isNotEmpty()->"partial";idle->"stopped";else->"unavailable"}
        val primary=record.optString("primary_id").takeIf{it.isNotEmpty()&&it!="null"}
        val web=primary?.let{applicationWeb(it)}
        val fingerprint=buildString{
            append(db.applicationControlEpoch()).append('|').append(target).append('|').append(record.getLong("version")).append('|').append(changing)
            restartPending.sortedBy{it.generation}.forEach{append('|').append(it.id).append(':').append(it.sourceId).append(':').append(it.generation)}
            bootPending.sortedBy{it.id}.forEach{append('|').append(it.id).append(':').append(it.version)}
            (0 until members.length()).map{members.getJSONObject(it)}.sortedBy{it.getString("id")}.forEach{member->
                val id=member.getString("id");val job=jobs[id]
                append('|').append(id).append(':').append(member.getLong("version")).append(':').append(member.getString("state"))
                    .append(':').append(job?.generation).append(':').append(job?.ready).append(':').append(job?.stopping).append(':').append(id in dataOwners)
                if(!group)append(':').append(db.runAuthority(id))
            }
        }
        val token=java.security.MessageDigest.getInstance("SHA-256").digest(fingerprint.toByteArray(Charsets.UTF_8)).joinToString(""){"%02x".format(it.toInt() and 255)}
        return record.put("state",state).put("state_token",token)
            .put("can_start",!changing&&!stopping&&!starting&&state in setOf("stopped","partial"))
            .put("can_stop",!changing&&!stopping&&(live.isNotEmpty()||automaticPending)).put("can_open",!changing&&web!=null)
            .put("has_failure",(0 until members.length()).any{members.getJSONObject(it).getString("state")=="dead"})
    }
    private fun applicationWeb(id:String):JSONObject?{
        val job=jobs[id]?.takeIf{it.ready&&!it.stopping}?:return null
        val labels=JSONObject(job.spec).optJSONObject("labels")?:return null
        val port=labels.optString("dev.wasmdroid.web.port").toIntOrNull()?:return null
        val path=labels.optString("dev.wasmdroid.web.path","/")
        if(!path.startsWith('/')||path.startsWith("//")||path.contains('\\')||path.any{it.isISOControl()})return null
        val binding=(0 until job.ports.length()).map{job.ports.getJSONObject(it)}.firstOrNull{it.getInt("instance_port")==port&&it.getString("protocol")=="tcp"}?:return null
        return JSONObject().put("id",id).put("generation",job.generation).put("instance_port",port).put("host_port",binding.getInt("host_port")).put("path",path)
    }
    fun shortcutOpen(targetJson:String,expectedStateToken:String,selectionCurrent:()->Boolean):CompletableFuture<String> = submit{db->
        check(selectionCurrent()){"Shortcut selection changed"}
        val record=applicationDescriptor(db,JSONObject(db.resolveApplicationTarget(targetJson)))
        check(record.getString("state_token")==expectedStateToken&&record.getBoolean("can_open")){"Application state changed; review it again"}
        check(selectionCurrent()){"Shortcut selection changed"}
        applicationWeb(record.getString("primary_id"))!!.put("name",record.getString("name")).toString()
    }
    fun shortcutAction(targetJson:String,action:String,expectedStateToken:String,selectionCurrent:()->Boolean):CompletableFuture<String> = submit{db->
        require(action in setOf("start","stop")){"Unknown application action"}
        check(selectionCurrent()){"Shortcut selection changed"}
        var record=applicationDescriptor(db,JSONObject(db.resolveApplicationTarget(targetJson)))
        check(record.getString("state_token")==expectedStateToken&&record.getBoolean("can_$action")){"Application state changed; review it again"}
        if(action=="start"){
            val members=record.getJSONArray("members")
            for(index in 0 until members.length()){
                val member=members.getJSONObject(index);if(member.getString("state")!="dead")continue
                check(selectionCurrent()){"Shortcut selection changed"}
                val old=member.getString("id");check(old !in jobs&&old !in dataOwners){"Service is still owned"}
                cancelAutomation(old);suppressDesiredRun(old)
                val replacement="app-${UUID.randomUUID()}"
                if(record.getJSONObject("target").getString("kind")=="group"){
                    val group=record.getJSONObject("target").getString("id")
                    val version=JSONObject(db.inspectStack(group)).getLong("version").toULong()
                    db.retryStackMember(group,version,member.getString("service"),member.getLong("version").toULong(),replacement)
                }else db.retryDeadInstance(old,member.getLong("version").toULong(),replacement)
                recentExits.remove(old)
            }
            record=applicationDescriptor(db,JSONObject(db.resolveApplicationTarget(targetJson)))
        }
        val admitted=record.getString("state_token")
        val guard={
            check(selectionCurrent()){"Shortcut selection changed"}
            check(applicationDescriptor(db,JSONObject(db.resolveApplicationTarget(targetJson))).getString("state_token")==admitted){"Application state changed; review it again"}
        }
        val target=record.getJSONObject("target")
        if(target.getString("kind")=="group"){
            if(action=="start")startStack(target.getString("id"),null,guard)
            else stopStack(target.getString("id"),guard).thenApply{"{}"}
        }else if(action=="start")start(target.getString("id"),guard)
        else stop(target.getString("id"),5_000,guard)
    }.thenCompose{it}
    fun removeApplicationTarget(targetJson:String,expectedStateToken:String):CompletableFuture<Unit> = submit{db->
        val record=applicationDescriptor(db,JSONObject(db.resolveApplicationTarget(targetJson)))
        check(record.getString("state_token")==expectedStateToken&&record.getString("state")=="stopped"){"Application state changed; review it again"}
        val guard={check(applicationDescriptor(db,JSONObject(db.resolveApplicationTarget(targetJson))).getString("state_token")==expectedStateToken){"Application state changed; review it again"}}
        val target=record.getJSONObject("target")
        if(target.getString("kind")=="group")removeStack(target.getString("id"),record.getLong("version").toULong(),guard)
        else remove(target.getString("id"),guard)
    }.thenCompose{it}
    private fun refreshApplicationSurfaces(){dev.wasmdroid.app.shortcuts.ShortcutWidgets.refreshAll(context)}
    private fun sharingEndpoint(record:JSONObject):LanSharingCoordinator.Endpoint? {
        val target=record.getJSONObject("target")
        val id=if(target.getString("kind")=="application")target.getString("id")
            else record.optString("primary_id").takeIf{it.isNotEmpty()&&it!="null"}?:return null
        return sharingEndpoint(id)
    }
    private fun sharingEndpoint(id:String):LanSharingCoordinator.Endpoint? {
        val web=applicationWeb(id)?:return null
        return LanSharingCoordinator.Endpoint(id,web.getString("generation"),web.getInt("instance_port"),web.getString("path"))
    }
    internal fun sharingPermissionChanged() {
        actor.post { lanSharing.admissionChanged() }
    }
    fun sharingStatus(targetJson:String):CompletableFuture<String> = submit{db->
        val record=applicationDescriptor(db,JSONObject(db.resolveApplicationTarget(targetJson)))
        lanSharing.status(sharingEndpoint(record)).thenApply{frame->JSONObject(frame).put("state_token",record.getString("state_token"))
            .put("target",record.getJSONObject("target")).toString()}
    }.thenCompose{it}
    internal fun serviceSharingStatus(id: String, generation: String): CompletableFuture<String> = submit {
        val endpoint = sharingEndpoint(id)?.takeIf { it.generation == generation }
        lanSharing.status(endpoint)
    }.thenCompose { it }
    internal fun startSharing(targetJson:String,stateToken:String,selectionToken:String,admission:()->Boolean,startIntent:LanStartIntent):CompletableFuture<String> = submit{db->
        val record=applicationDescriptor(db,JSONObject(db.resolveApplicationTarget(targetJson)))
        check(record.getString("state_token")==stateToken&&admission()){"Sharing application state changed; review it again"}
        val endpoint=sharingEndpoint(record)?:error("Start the application's web service before sharing")
        lanSharing.enable(endpoint,selectionToken,admission,startIntent)
    }.thenCompose{it}
    fun stopSharing(targetJson:String,grantEpoch:Long):CompletableFuture<String> = submit{db->
        val record=applicationDescriptor(db,JSONObject(db.resolveApplicationTarget(targetJson)))
        val endpoint=sharingEndpoint(record)?:error("The application is stopping or no longer running")
        lanSharing.disable(endpoint,grantEpoch)
    }.thenCompose{it}
    fun acquireBrowserSession(id:String,generation:String,guestPort:Int,path:String,strict:Boolean=false):CompletableFuture<BrowserSessionLease> = submit{db->
        val job=jobs[id]?:error("Application is not running")
        check(job.ready&&!job.stopping&&job.generation==generation){"Browser generation changed"}
        val identity=db.browserIdentity(id)
        val lease=browserSessions.acquire(id,generation,guestPort,path,strict)
        if(lease.profileId!=identity){lease.close();error("Browser profile identity changed")}
        lease
    }
    fun browserProfileState(id:String):CompletableFuture<String> = submit{db->
        val identity=db.browserIdentity(id)
        JSONObject(BrowserProfiles.get(context).state(identity))
            .put("profile_token",BrowserProfileLedger.profileName(identity)).toString()
    }
    fun browserCapabilities():CompletableFuture<String> = submit{BrowserProfiles.get(context).capabilities()}
    fun eraseBrowserProfile(id:String,expectedRevision:Long,expectedProfileToken:String):CompletableFuture<String> = submit{db->
        val profileId=db.browserIdentity(id)
        check(BrowserProfileLedger.profileName(profileId)==expectedProfileToken){"Browser profile changed; review its data again"}
        lifetime.beginBackground()
        notifyActiveCount()
        val answer=CompletableFuture<String>()
        try{
            // Provider acknowledgement remains private and retained. Only its
            // actual callback can release erasure ownership, never the caller.
            BrowserProfiles.get(context).erase(profileId,expectedRevision).whenComplete{value,failure->
                lifetime.endBackground();actor.post{notifyActiveCount();finishCloseIfIdle()}
                if(failure==null)answer.complete(value)else answer.completeExceptionally(failure)
            }
        }catch(failure:Exception){
            lifetime.endBackground();finishCloseIfIdle();answer.completeExceptionally(failure)
        }
        answer
    }.thenCompose{it}
    private fun recordAutomaticFailure(id:String,kind:String){
        automaticFailures[id]=kind
        while(automaticFailures.size>32)automaticFailures.remove(automaticFailures.keys.first())
    }
    private fun storedEpoch(authority:JSONObject):ULong? =
        if(authority.isNull("boot_epoch"))null else authority.getLong("boot_epoch").toULong()
    private fun authorityRecord(id:String,record:JSONObject,authority:JSONObject,managed:Boolean=false,owned:Boolean=false):ServiceRestartCoordinator.Record {
        val state=when(record.getString("state")){
            "created"->ServiceRestartCoordinator.State.CREATED
            "exited"->ServiceRestartCoordinator.State.EXITED
            "dead"->ServiceRestartCoordinator.State.DEAD
            else->error("Service is not terminal")
        }
        return ServiceRestartCoordinator.Record(id,record.getLong("version").toULong(),state,
            ServiceRestartCoordinator.Policy.parse(record.getJSONObject("spec").optString("restart_policy","no")),
            managed,owned,authority.getLong("revision").toULong(),authority.getBoolean("desired_run"),storedEpoch(authority)==bootEpoch)
    }
    /** User intent is durable before any signal or conflicting metadata change.
     * A failed write fails the action while the old worker stays owned. */
    private fun suppressDesiredRun(id:String,generation:String?=null){
        val db=storage.join()
        val record=JSONObject(db.inspectInstance(id));val authority=JSONObject(db.runAuthority(id))
        val currentGeneration=if(authority.isNull("generation"))null else authority.getString("generation")
        check(generation==null||currentGeneration==null||generation==currentGeneration){"Run generation changed"}
        val saved=JSONObject(db.stopRunIntent(id,record.getLong("version").toULong(),authority.getLong("revision").toULong(),currentGeneration))
        jobs[id]?.authority=saved
    }
    private fun restartRecord(id:String):ServiceRestartCoordinator.Record?{
        if(closing||!lifetime.admits())return null
        val db=storage.join();val record=JSONObject(db.inspectInstance(id))
        if(record.getString("state") !in listOf("created","exited","dead"))return null
        return authorityRecord(id,record,JSONObject(db.runAuthority(id)),db.instanceStack(id)!=null,id in jobs||id in dataOwners)
    }
    /** A new owner enumerates only same-boot durable native candidates. Calling
     * this again on the same owner never creates fresh authorization. */
    fun recoverAuthorizedApplications():CompletableFuture<String>{
        val answer=CompletableFuture<String>()
        if(!actor.post{
            recoveryResult?.let{previous->previous.whenComplete{value,failure->if(failure==null)answer.complete(value)else answer.completeExceptionally(failure)};return@post}
            recoveryResult=answer
            try{
                check(!closing&&lifetime.admits()){"Supervisor is closing or retiring"}
                if(bootEpoch==null){
                    answer.complete(JSONObject().put("scheduled",0).put("blocked","boot_identity_unavailable").toString());return@post
                }
                val scan=RecoveryScan(answer);recoveryScan=scan;scanRecoveryPage(scan)
            }catch(failure:Exception){answer.completeExceptionally(failure)}
            finally{notifyActiveCount()}
        })answer.completeExceptionally(IllegalStateException("Supervisor is closed"))
        return answer
    }
    private fun recoveryReport(scan:RecoveryScan,complete:Boolean,capacity:Boolean=false,cancelled:Boolean=false):String = JSONObject()
        .put("scheduled",scan.scheduled).put("skipped",scan.skipped).put("scanned",scan.scanned)
        .put("complete",complete).put("capacity_blocked",capacity).put("cancelled",cancelled)
        .put("next_cursor",scan.cursor?:JSONObject.NULL).toString()
    private fun scanRecoveryPage(scan:RecoveryScan){
        if(recoveryScan!==scan)return
        try{
            check(!closing&&lifetime.admits()){"Supervisor is closing or retiring"}
            val page=JSONObject(storage.join().recoveryCandidates(bootEpoch,scan.cursor,100u))
            val items=page.getJSONArray("items");scan.scanned+=page.getLong("scanned");scan.skipped+=page.getInt("skipped")
            for(index in 0 until items.length()){
                val candidate=items.getJSONObject(index);val id=candidate.getString("id");val authority=candidate.getJSONObject("authority")
                // Pending tickets reserve capacity as well as live Jobs. Any
                // remainder is explicit in the receipt, never silent first-N.
                if(jobs.size+restarts.pendingCount+pendingBoot.size>=slots.size){
                    recoveryScan=null;scan.answer.complete(recoveryReport(scan,false,true));notifyActiveCount();return
                }
                val current=restartRecord(id)
                if(current!=null&&current.version==candidate.getLong("version").toULong()&&current.authorityRevision==authority.getLong("revision").toULong()&&
                    restarts.recover(current,authority.getString("generation")))scan.scheduled++ else scan.skipped++
                scan.cursor=id
            }
            val next=if(page.isNull("next_cursor"))null else page.getString("next_cursor")
            if(next!=null){
                scan.cursor=next
                check(actor.post{scanRecoveryPage(scan)}){"Supervisor is closed"}
            }else{
                recoveryScan=null;scan.cursor=null;scan.answer.complete(recoveryReport(scan,true));notifyActiveCount()
            }
        }catch(failure:Exception){recoveryScan=null;scan.answer.completeExceptionally(failure);notifyActiveCount()}
    }
    private fun cancelAutomation(id:String){
        restarts.cancel(id)
        val cancelled=pendingBoot.filter{(source,candidate)->source==id||candidate.id==id}.keys
        cancelled.forEach{pendingBoot.remove(it);bootRun?.cancelled?.add(it)}
        bootRun?.active?.let{(source,current)->if(source==id||current==id)bootRun?.cancelled?.add(source)}
        automaticFailures.remove(id)
        if(cancelled.isNotEmpty())notifyActiveCount()
    }
    private fun cancelAllAutomation(){
        restarts.cancelAll()
        recoveryScan?.let{scan->recoveryScan=null;scan.answer.complete(recoveryReport(scan,false,cancelled=true))}
        val run=bootRun
        if(run!=null){
            run.skipped+=(run.candidates.size-run.position)+(if(run.active!=null)1 else 0)
            pendingBoot.clear();bootRun=null
            run.result.complete(bootReport(run,true))
        }
        notifyActiveCount()
    }
    private fun launchRestart(original:ServiceRestartCoordinator.Ticket){
        var ticket=original
        try{
            val db=storage.join()
            check(restarts.valid(ticket,restartRecord(ticket.id))){"Automatic restart admission changed"}
            check(slots.indices.any{slot->jobs.values.none{it.slot==slot}}){"No free execution worker"}
            if(ticket.state==ServiceRestartCoordinator.State.DEAD){
                val replacement="app-${UUID.randomUUID()}"
                db.retryDeadInstance(ticket.id,ticket.version,replacement)
                recentExits.remove(ticket.id)
                val record=restartRecord(replacement)?:error("Restart replacement is unavailable")
                ticket=restarts.replaced(ticket,record)
            }
            check(restarts.valid(ticket,restartRecord(ticket.id))){"Automatic restart admission changed"}
            reserveStandalone(db,ticket.id,JSONObject(db.inspectInstance(ticket.id)),ticket,if(ticket.recovery)"recovery" else "restart")
        }catch(failure:Exception){restarts.admissionFailed(ticket,failure)}
    }

    /** Boot authorization comes only from the current native specification.
     * Queued candidates retain reservations and are checked on this actor both
     * before versioned replacement and before the actual worker reservation. */
    fun restoreBootApplications():CompletableFuture<String>{
        val answer=CompletableFuture<String>()
        if(!actor.post{
            val previous=bootResult
            if(previous!=null){previous.whenComplete{value,failure->if(failure==null)answer.complete(value)else answer.completeExceptionally(failure)};return@post}
            bootResult=answer
            try{
                check(!closing&&lifetime.admits()){"Supervisor is closing or retiring"}
                val db=storage.join();val raw=db.listInstances("local");val records=JSONArray(raw)
                val managed=(0 until records.length()).map{records.getJSONObject(it).getString("id")}.filter{db.instanceStack(it)!=null}.toSet()
                val candidates=BootApplicationPlan.candidates(raw,managed)
                val run=BootRun(candidates,answer);bootRun=run
                candidates.forEach{pendingBoot[it.id]=it}
                notifyActiveCount();advanceBoot(run)
            }catch(failure:Exception){pendingBoot.clear();bootRun=null;answer.completeExceptionally(failure);notifyActiveCount()}
        })answer.completeExceptionally(IllegalStateException("Supervisor is closed"))
        return answer
    }
    private fun bootReport(run:BootRun,cancelled:Boolean=false):String = JSONObject()
        .put("started",run.started).put("failed",run.failed).put("skipped",run.skipped).put("cancelled",cancelled).toString()
    private fun advanceBoot(run:BootRun){
        if(bootRun!==run)return
        if(closing||!lifetime.admits()){cancelAllAutomation();return}
        while(run.position<run.candidates.size){
            val source=run.candidates[run.position++].id
            var candidate=pendingBoot[source]
            if(candidate==null){run.skipped++;continue}
            try{
                val db=storage.join()
                if(!BootApplicationPlan.valid(candidate,db.inspectInstance(candidate.id),db.instanceStack(candidate.id)!=null,candidate.id in jobs||candidate.id in dataOwners)){
                    pendingBoot.remove(source);run.skipped++;continue
                }
                check(slots.indices.any{slot->jobs.values.none{it.slot==slot}}){"No free execution worker"}
                if(candidate.replaceDead){
                    val replacement="app-${UUID.randomUUID()}"
                    db.retryDeadInstance(candidate.id,candidate.version,replacement)
                    recentExits.remove(candidate.id)
                    val record=JSONObject(db.inspectInstance(replacement))
                    candidate=BootApplicationPlan.Candidate(replacement,record.getLong("version").toULong(),false)
                    pendingBoot[source]=candidate
                }
                check(pendingBoot[source]===candidate&&BootApplicationPlan.valid(candidate,db.inspectInstance(candidate.id),db.instanceStack(candidate.id)!=null,candidate.id in jobs||candidate.id in dataOwners)){
                    "Boot admission changed"
                }
                val admitted=candidate
                // Reserve the Job before dropping the boot reservation.
                val job=reserveStandalone(db,admitted.id,JSONObject(db.inspectInstance(admitted.id)),source="boot")
                pendingBoot.remove(source);run.active=source to admitted.id;notifyActiveCount()
                job.started.whenComplete{_,failure->actor.post{
                    if(bootRun===run){
                        run.active=null
                        if(source in run.cancelled)run.skipped++
                        else if(failure==null)run.started++
                        else{run.failed++;recordAutomaticFailure(admitted.id,"boot_start_failed")}
                        advanceBoot(run)
                    }
                }}
                return
            }catch(failure:Exception){pendingBoot.remove(source);run.failed++;recordAutomaticFailure(candidate?.id?:source,"boot_admission_failed")}
        }
        bootRun=null;notifyActiveCount();run.result.complete(bootReport(run))
    }

    fun images(): CompletableFuture<String> = disk { it.listImages() }
    /** One actual bounded scan at a time. Each caller receives a separate
     * observer: cancellation/completion never acknowledges or releases IO. */
    fun storageUsage(): CompletableFuture<String> = synchronized(storageUsageLock) {
        val work = storageUsageWork ?: disk { db ->
            JSONObject(db.storageUsage()).apply {
                // Filesystem availability is sampled separately from native
                // logical-byte accounting; neither is a reservation or quota.
                try {
                    val available = StatFs(context.filesDir.absolutePath).availableBytes
                    check(available >= 0)
                    put("available_bytes", available)
                    put("available_status", "available")
                } catch (_: Exception) {
                    put("available_bytes", JSONObject.NULL)
                    put("available_status", "unavailable")
                }
            }.toString()
        }.also { captured ->
            storageUsageWork = captured
            captured.whenComplete { _, _ ->
                synchronized(storageUsageLock) {
                    if (storageUsageWork === captured) storageUsageWork = null
                }
            }
        }
        CompletableFuture<String>().also { observer ->
            work.whenComplete { value, failure ->
                if (failure == null) observer.complete(value)
                else observer.completeExceptionally(failure)
            }
        }
    }
    fun catalogState():CompletableFuture<String> = disk{catalogUpdates.state(it)}
    fun refreshCatalog():CompletableFuture<String> = submit{catalogUpdates.refresh().also{notifyActiveCount()}}.thenCompose{it}
    fun cancelCatalogRefresh(generation:String):CompletableFuture<Boolean> = submit{catalogUpdates.cancel(generation)}
    fun installRemoteCatalog(pinJson:String,label:String,maxStoreBytes:ULong?=null):CompletableFuture<String>{
        require(pinJson.toByteArray(Charsets.UTF_8).size<=1024){"Catalog pin exceeds input limit"}
        return packageAction("install","application",label,{installRemoteCatalog(pinJson,label,maxStoreBytes)},null){db,operation,progress,budget->
            val entry=JSONObject(catalogUpdates.packageForInstall(db,pinJson))
            db.validateCatalogApplication(entry.toString())
            val graph=db.installCatalogPackageControlled(entry.toString(),budget,maxStoreBytes,operation.control,progress)
            operation.stage.set("preparing_app")
            prepareCatalogApplication(db,entry,JSONObject(graph))
        }
    }
    fun importPackage(uri:android.net.Uri):CompletableFuture<String> =
        submit{localPackages.begin(DocumentPackageInput(context.contentResolver,uri))}
    internal fun importPackageInput(input:LocalPackageInput):CompletableFuture<String> = submit{localPackages.begin(input)}
    fun localPackage(key:String,generation:String):CompletableFuture<String> = submit{localPackages.frame(key,generation)}
    fun reviewLocalPackage(key:String,generation:String):CompletableFuture<Unit> = submit{localPackages.review(key,generation)}
    fun commitLocalPackage(key:String,generation:String,token:String):CompletableFuture<Unit> = submit{localPackages.commit(key,generation,token)}
    fun removeImage(reference:String):CompletableFuture<String> = disk{it.removeImage(reference)}
    fun collectUnusedCache():CompletableFuture<String> = disk{it.collectUnusedCache()}
    fun pull(reference:String,observer:ProbeObserver?=null,maxStoreBytes:ULong?=null):CompletableFuture<String> =
        packageAction("pull:$reference","image",reference,{pull(reference,null,maxStoreBytes)},observer){db,operation,progress,budget->
            db.pullImageControlled(reference,budget,maxStoreBytes,operation.control,progress)
        }
    fun installCatalog(packageJson:String,observer:ProbeObserver?=null,maxStoreBytes:ULong?=null):CompletableFuture<String> {
        if (packageJson.toByteArray(Charsets.UTF_8).size > 128 * 1024) {
            return CompletableFuture<String>().apply {
                completeExceptionally(IllegalArgumentException("Catalog recipe exceeds input limit"))
            }
        }
        return packageAction("install","catalog_image","",{installCatalog(packageJson,null,maxStoreBytes)},observer){db,operation,progress,budget->
            db.installCatalogPackageControlled(packageJson,budget,maxStoreBytes,operation.control,progress)
        }
    }
    fun installApplication(packageJson:String,label:String,maxStoreBytes:ULong?=null):CompletableFuture<String> {
        if (packageJson.toByteArray(Charsets.UTF_8).size > 128 * 1024) {
            return CompletableFuture<String>().apply {
                completeExceptionally(IllegalArgumentException("Catalog recipe exceeds input limit"))
            }
        }
        return packageAction("install","application",label,{installApplication(packageJson,label,maxStoreBytes)},null){db,operation,progress,budget->
            val entry=JSONObject(packageJson)
            db.validateCatalogApplication(entry.toString())
            val graph=db.installCatalogPackageControlled(packageJson,budget,maxStoreBytes,operation.control,progress)
            operation.stage.set("preparing_app")
            prepareCatalogApplication(db,entry,JSONObject(graph))
        }
    }
    /** A stale UI cancellation cannot target a later installation with the same key. */
    fun cancelPackageOperation(key:String,generation:String):Boolean{
        val operation=packageOperations[key]?.takeIf{it.generation==generation}?:return false
        val accepted=runCatching{operation.control.cancel()}.getOrDefault(false)
        if(accepted&&operation.kind=="local_import")actor.post{localPackages.cancelled(key,generation)}
        return accepted
    }
    fun packageOperations():String=JSONObject().apply{
        packageOperations.forEach{(key,operation)->runCatching{operation.snapshot()}.getOrNull()?.let{put(key,it)}}
    }.toString()
    fun packageActivity():CompletableFuture<String> = submit{
        JSONObject().put("active",JSONObject(packageOperations()))
            .put("recent",JSONArray(recentPackages.values.map{JSONObject(it.json)})).toString()
    }
    fun dismissPackage(key:String,generation:String):CompletableFuture<Unit> = submit{
        if(recentPackages[key]?.let{JSONObject(it.json).getString("generation")==generation}==true)recentPackages.remove(key)
        localPackages.forget(key,generation)
        Unit
    }
    fun retryPackage(key:String,generation:String):CompletableFuture<String>{
        val result=CompletableFuture<String>()
        val posted=actor.post{
            try{
                check(!closing&&lifetime.admits()&&!packageOperations.containsKey(key)){"Package operation is already active or supervisor is closing"}
                val receipt=recentPackages[key]?:error("Package outcome is no longer retained")
                val record=JSONObject(receipt.json)
                check(record.getString("generation")==generation&&record.getString("status") in listOf("failed","cancelled")){"Package outcome changed"}
                receipt.retry().whenComplete{value,failure->if(failure==null)result.complete(value)else result.completeExceptionally(failure)}
            }catch(failure:Exception){result.completeExceptionally(failure)}
        }
        if(!posted)result.completeExceptionally(IllegalStateException("Supervisor is closed"))
        return result
    }
    private fun packageAction(key:String,kind:String,label:String,retry:()->CompletableFuture<String>,observer:ProbeObserver?,action:(EngineStorage,PackageOperation,ProbeObserver,ULong)->String):CompletableFuture<String>{
        val answer=CompletableFuture<String>()
        val posted=actor.post{
            if(closing||!lifetime.admits()||packageOperations.containsKey(key)){answer.completeExceptionally(IllegalStateException("Package operation is already active or supervisor is closing"));return@post}
            if(packageOperations.size>=16){answer.completeExceptionally(IllegalStateException("Too many package operations"));return@post}
            val operation=PackageOperation(key,kind,label,InstallControl(),retry)
            packageOperations[key]=operation;recentPackages.remove(key);notifyActiveCount()
            val progress=object:ProbeObserver{override fun onPhase(phase:String){operation.progress(phase);observer?.onPhase(phase)}}
            try{
                CompletableFuture.supplyAsync({
                    val available=context.filesDir.availableTransferBytes().toULong()
                    action(storage.join(),operation,progress,available)
                },io).whenComplete{value,failure->actor.post{
                    val native=operation.control.status()
                    val status=when{failure==null->"installed";native==dev.wasmdroid.engine.InstallStatus.CANCELLED->"cancelled";else->"failed"}
                    val receipt=operation.snapshot(status).put("image_committed",native==dev.wasmdroid.engine.InstallStatus.COMMITTED)
                    failure?.let{val (errorKind,message)=packageFailure(it);receipt.put("error_kind",errorKind).put("technical_error",message)}
                    recentPackages[key]=PackageReceipt(receipt.toString(),retry)
                    while(recentPackages.size>16)recentPackages.remove(recentPackages.keys.first())
                    packageOperations.remove(key,operation);operation.control.close()
                    refreshApplicationSurfaces()
                    if(failure==null)answer.complete(value)else answer.completeExceptionally(PackageActionFailure(key,operation.generation,failure))
                    finishCloseIfIdle()
                }}
            }catch(failure:Exception){
                packageOperations.remove(key,operation);operation.control.close();answer.completeExceptionally(failure);finishCloseIfIdle()
            }
        }
        if(!posted)answer.completeExceptionally(IllegalStateException("Supervisor is closed"))
        return answer
    }
    fun search(query: String, cursor: String?, exactRelease: String?): CompletableFuture<String> =
        disk { it.searchRegistry(query, cursor, exactRelease) }
    fun volumes(): CompletableFuture<String> = submit { it.listVolumes() }
    fun createVolume(name: String): CompletableFuture<String> = submit { it.createVolume(name) }
    fun createNewVolume(name:String):CompletableFuture<String> = submit{it.createNewVolume(name)}
    fun removeVolume(name: String): CompletableFuture<Unit> = submit { it.removeVolume(name) }
    fun instances(): CompletableFuture<String> = submit { it.listInstances("local") }
    /** Small actual-membership lookup; UI labels never decide group ownership. */
    fun memberships(ids:List<String>):CompletableFuture<String> = submit { db->
        JSONObject().apply{ids.forEach{id->db.instanceStack(id)?.let{put(id,it)}}}.toString()
    }
    fun previewStack(id:String,document:String):CompletableFuture<String> = disk{it.previewStack(id,document)}
    fun stackPackageRequirements(document:String):CompletableFuture<String> = disk{it.stackPackageRequirements(document)}
    fun createStack(id:String,document:String):CompletableFuture<String> = disk{it.createStack(id,document)}
    fun createReviewedStack(id:String,preview:String):CompletableFuture<String> = disk{it.createStackFromPreview(id,preview)}.thenApply{value->refreshApplicationSurfaces();value}
    fun stackEditSource(id:String):CompletableFuture<String> = disk{it.stackEditSource(id)}
    fun previewStackUpdate(id:String,version:ULong,document:String):CompletableFuture<String> = disk{it.previewStackUpdate(id,version,document)}
    /** Native preflight yields only actual member identities/revisions on IO.
     * The actor reserves ownership before the independently revalidated commit. */
    fun updateReviewedStack(id:String,preview:String):CompletableFuture<String> {
        require(preview.toByteArray(Charsets.UTF_8).size<=128*1024){"Stack update exceeds IPC budget"}
        return disk{it.validateStackUpdatePreview(id,preview)}.thenCompose{admission->
            submit{db->
                check(id !in stackStarts&&id !in stackEdits){"Stack operation is still active"}
                check(jobs.values.none{it.stackId==id}){"Stop all stack services before updating the definition"}
                val members=JSONObject(admission)
                val previous=members.keys().asSequence().map{members.getJSONObject(it)}.associate{it.getString("instance_id") to it.getLong("version")}
                check(previous.keys.none{it in jobs||it in dataOwners}){"Stop stack services and finish data operations before updating the definition"}
                val token=UUID.randomUUID().toString()
                previous.keys.forEach{cancelAutomation(it)}
                stackEdits[id]=token;previous.keys.forEach{dataOwners[it]=token};notifyActiveCount()
                val answer=CompletableFuture<String>()
                fun release(){
                    if(stackEdits[id]==token)stackEdits.remove(id)
                    previous.keys.forEach{if(dataOwners[it]==token)dataOwners.remove(it)}
                    refreshApplicationSurfaces()
                    finishCloseIfIdle()
                }
                try{
                    CompletableFuture.supplyAsync({db.updateStackFromPreview(id,preview)},io).whenComplete{value,failure->actor.post{
                        try{
                            if(failure==null){
                                runCatching{
                                    val instances=JSONObject(value).getJSONArray("instances")
                                    val current=(0 until instances.length()).map{instances.getJSONObject(it)}.associate{it.getString("id") to it.getLong("version")}
                                    previous.forEach{(member,version)->if(current[member]!=version)recentExits.remove(member)}
                                }
                                answer.complete(value)
                            }else answer.completeExceptionally(failure)
                        }catch(failure:Exception){answer.completeExceptionally(failure)}finally{release()}
                    }}
                }catch(failure:Exception){answer.completeExceptionally(failure);release()}
                answer
            }.thenCompose{it}
        }
    }
    fun stacks(after:String?=null,limit:UInt=100u):CompletableFuture<String> = submit{it.listStacks(after,limit)}
    fun inspectStack(id:String):CompletableFuture<String> = submit{it.inspectStack(id)}
    fun stackMembers(id:String):CompletableFuture<String> = submit{it.stackMembers(id)}
    fun stackSnapshot(id:String):CompletableFuture<String> = submit{db->
        JSONObject(db.stackSummary(id)).put("members",JSONArray(db.stackMembers(id))).toString()
    }
    fun removeStack(id:String,version:ULong,admission:(()->Unit)?=null):CompletableFuture<Unit> = submit{db->
        admission?.invoke()
        check(id !in stackStarts&&id !in stackEdits){"Stack operation is still active"}
        val members=JSONArray(db.stackMembers(id))
        for(index in 0 until members.length()){
            val member=members.getJSONObject(index).getString("instance_id")
            check(member !in jobs&&member !in dataOwners){"Stop stack services and finish data operations before removal"}
        }
        db.removeStack(id,version)
        refreshApplicationSurfaces()
        for(index in 0 until members.length()){
            val member=members.getJSONObject(index).getString("instance_id")
            cancelAutomation(member);recentExits.remove(member)
        }
        Unit
    }
    fun inspect(id: String): CompletableFuture<String> = submit { it.inspectInstance(id) }
    fun ports(id: String): CompletableFuture<String> = submit { it.instancePorts(id) }
    fun controllers(): CompletableFuture<String> = submit {
        JSONObject().apply {
            restarts.pendingTickets().forEach{put(it.id,"restarting")}
            pendingBoot.values.forEach{put(it.id,"starting")}
            dataOwners.keys.forEach{put(it,"data")}
            jobs.values.forEach { job -> put(job.id,when {job.stopping->"stopping";job.ready->"running";else->"starting"})}
        }.toString()
    }
    fun beginDataOperation(id:String):CompletableFuture<InstanceDataLease> = submit { db->
        cancelAutomation(id)
        check(!jobs.containsKey(id)&&id !in dataOwners){"Stop service before managing its data"}
        suppressDesiredRun(id)
        db.instanceStack(id)?.let{check(it !in stackEdits){"Stack definition update is still active"}}
        val record=db.inspectInstance(id)
        check(JSONObject(record).getString("state") !in listOf("running","paused","restarting")){"Stop service before managing its data"}
        val token=UUID.randomUUID().toString();dataOwners[id]=token
        notifyActiveCount()
        refreshApplicationSurfaces()
        InstanceDataLease(record){endDataOperation(id,token)}
    }
    private fun endDataOperation(id:String,token:String):CompletableFuture<Unit> {
        val result=CompletableFuture<Unit>()
        if(!actor.post{if(dataOwners[id]==token)dataOwners.remove(id);refreshApplicationSurfaces();result.complete(Unit);finishCloseIfIdle()})result.complete(Unit)
        return result
    }
    fun dataEntries(volume:String,path:String):CompletableFuture<String> = disk{it.volumeEntries(volume,path)}
    fun createDirectory(volume:String,path:String):CompletableFuture<Unit> = disk{it.createDataDirectory(volume,path)}
    fun removeEntry(volume:String,path:String,directory:Boolean):CompletableFuture<Unit> = disk{it.removeDataEntry(volume,path,directory)}
    fun beginImport(volume:String,path:String,overwrite:Boolean,expected:ULong?,budget:ULong,control:DataTransferControl):CompletableFuture<DataWriteTransfer> =
        disk{it.beginDataImport(volume,path,overwrite,expected,budget,control)}
    fun beginExport(volume:String,path:String):CompletableFuture<DataReadTransfer> = disk{it.beginDataExport(volume,path)}
    fun beginBackup(volume:String,control:DataTransferControl):CompletableFuture<DataReadTransfer> = disk{it.beginVolumeBackup(volume,ULong.MAX_VALUE,control)}
    fun beginRestore(name:String,archiveBudget:ULong,unpackedBudget:ULong,control:DataTransferControl):CompletableFuture<BackupRestoreTransfer> =
        disk{it.beginVolumeRestore(name,archiveBudget,unpackedBudget,control)}
    fun attachRestored(id:String,version:ULong,oldVolume:String,newVolume:String):CompletableFuture<String> =
        submit{db->
            cancelAutomation(id)
            check(id !in jobs){"Stop service before replacing its data"}
            db.instanceStack(id)?.let{check(it !in stackEdits){"Stack definition update is still active"}}
            suppressDesiredRun(id)
            db.attachRestoredVolume(id,version,oldVolume,newVolume)
        }
    fun events(after: ULong, limit: UInt = 100u): CompletableFuture<String> = submit { it.eventsAfter(after, limit) }
    fun output(id: String): CompletableFuture<String?> = submit { it.latestOutput(id) }
    fun logs(id:String,query:String):CompletableFuture<String?> = CompletableFuture.supplyAsync({storage.join().readLogs(id,query)},logIo)
    fun savedTerminal(id:String,generation:String,startRow:UShort):CompletableFuture<String?> = CompletableFuture.supplyAsync({storage.join().savedTerminal(id,generation,startRow)},logIo)
    fun terminalFrame(id:String,generation:String,query:String):CompletableFuture<String>{
        require(query.toByteArray(Charsets.UTF_8).size<=4096){"Terminal query exceeds limit"}
        return terminalCommand(id,generation,WorkerProtocol.TERMINAL_FRAME,Bundle().apply{putString(WorkerProtocol.QUERY,query)})
    }
    fun workerStats(id:String,generation:String):CompletableFuture<String> =
        terminalCommand(id,generation,WorkerProtocol.STATS)
    fun workerStats(id:String):CompletableFuture<String> = terminalSession(id).thenCompose { session ->
        val generation=JSONObject(session).getString("generation")
        workerStats(id,generation).thenApply { raw -> JSONObject(raw).put("generation",generation).toString() }
    }
    fun terminalSession(id:String):CompletableFuture<String> = submit {
        val job=jobs[id]?:error("Service is not running")
        check(job.ready&&!job.stopping){"Service is not ready for terminal attachment"}
        snapshot(job)
    }
    fun writeStdin(id:String,generation:String,bytes:ByteArray):CompletableFuture<Unit> {
        require(bytes.size<=4096){"Terminal input exceeds message limit"}
        return terminalCommand(id,generation,WorkerProtocol.STDIN,Bundle().apply{putByteArray(WorkerProtocol.BYTES,bytes)}).thenApply{Unit}
    }
    fun closeStdin(id:String,generation:String):CompletableFuture<Unit> = terminalCommand(id,generation,WorkerProtocol.STDIN_EOF).thenApply{Unit}
    fun interrupt(id:String,generation:String):CompletableFuture<Unit> = terminalCommand(id,generation,WorkerProtocol.INTERRUPT).thenApply{Unit}
    fun resizeTerminal(id:String,generation:String,cols:Int,rows:Int,width:Int,height:Int):CompletableFuture<Unit> =
        terminalCommand(id,generation,WorkerProtocol.RESIZE,Bundle().apply{putInt(WorkerProtocol.COLS,cols);putInt(WorkerProtocol.ROWS,rows);putInt(WorkerProtocol.WIDTH,width);putInt(WorkerProtocol.HEIGHT,height)}).thenApply{Unit}
    private fun terminalCommand(id:String,generation:String,what:Int,extras:Bundle=Bundle(),admission:(()->Boolean)?=null):CompletableFuture<String>{
        val answer=CompletableFuture<String>()
        actor.post{
            try{
                val job=jobs[id]?:error("Terminal service ended")
                check(job.generation==generation&&job.ready&&!job.stopping){"Terminal generation ended"}
                val stdin=what==WorkerProtocol.STDIN||what==WorkerProtocol.STDIN_EOF
                check(job.commands.size<4&&(!stdin||job.stdinCommand==null)){"Terminal control is busy"}
                check(admission?.invoke()!=false){"Worker control admission changed"}
                val request=UUID.randomUUID().toString();extras.putString(WorkerProtocol.REQUEST,request)
                job.commands[request]=answer;if(stdin)job.stdinCommand=request
                try{send(job,what,extras)}catch(failure:Exception){job.commands.remove(request);if(job.stdinCommand==request)job.stdinCommand=null;throw failure}
                val timeout=when(what){WorkerProtocol.LAN_ENABLE,WorkerProtocol.LAN_DISABLE->15_000L;WorkerProtocol.LAN_STATUS,WorkerProtocol.STATS,WorkerProtocol.NETWORK->3_000L;else->null}
                if(timeout!=null)actor.postDelayed({
                    if(isCurrent(job)&&job.commands.remove(request)===answer)
                        answer.completeExceptionally(IllegalStateException("Worker control timed out"))
                },timeout)
            }catch(failure:Exception){answer.completeExceptionally(failure)}
        }
        return answer
    }
    fun waitForExit(id: String): CompletableFuture<String> {
        val answer = CompletableFuture<String>()
        actor.post {
            val job = jobs[id]
            if (job != null) job.ended.whenComplete { value, failure ->
                if (failure == null) answer.complete(value) else answer.completeExceptionally(failure)
            } else try {
                val recent=recentExits[id]
                if(recent!=null)answer.complete(recent)
                else {
                    val record=JSONObject(storage.join().inspectInstance(id))
                    check(record.getString("state") in listOf("exited","dead")){"Service has no completed execution"}
                    // No current-owner receipt: do not attach a previous run's
                    // latest-by-mtime output or invent an escalation decision.
                    answer.complete(JSONObject().put("instance",record)
                        .put("generation",JSONObject.NULL).put("forced",JSONObject.NULL)
                        .put("guest_result",JSONObject.NULL).toString())
                }
            }
            catch (failure: Exception) { answer.completeExceptionally(failure) }
        }
        return answer
    }
    fun restart(id: String): CompletableFuture<String> = stop(id).thenCompose { start(id) }
    fun retry(id: String): CompletableFuture<String> = submit { db ->
        cancelAutomation(id)
        check(id !in dataOwners){"Data operation is still active"}
        check(!jobs.containsKey(id)) { "Service still owns a worker" }
        suppressDesiredRun(id)
        val old=JSONObject(db.inspectInstance(id))
        val replacement="app-${UUID.randomUUID()}"
        val stackId=db.instanceStack(id)
        if(stackId==null)db.retryDeadInstance(id,old.getLong("version").toULong(),replacement)
        else{
            check(stackId !in stackStarts&&stackId !in stackEdits){"Stack operation is still active"}
            val definition=JSONObject(db.stackPlan(stackId));val members=JSONArray(db.stackMembers(stackId))
            val member=(0 until members.length()).map{members.getJSONObject(it)}.first{it.getString("instance_id")==id}
            db.retryStackMember(stackId,definition.getLong("version").toULong(),member.getString("service"),old.getLong("version").toULong(),replacement)
            recentExits.remove(id)
        }
        replacement
    }.thenCompose { start(it) }
    fun create(id: String, spec: String): CompletableFuture<String> = submit {
        require(spec.toByteArray(Charsets.UTF_8).size <= 128 * 1024) { "Service specification too large" }
        it.createInstance(id, spec).also{refreshApplicationSurfaces()}
    }
    /** Actor fencing covers Created-but-starting and exited-but-not-yet-dead
     * workers as well as data operations; DB state alone is not this proof. */
    fun configure(id:String,version:ULong,spec:String):CompletableFuture<String> = submit{db->
        cancelAutomation(id)
        check(id !in jobs&&id !in dataOwners){"Stop service and finish data operations before configuration"}
        suppressDesiredRun(id)
        require(spec.toByteArray(Charsets.UTF_8).size<=128*1024){"Service specification too large"}
        val token=UUID.randomUUID().toString()
        dataOwners[id]=token
        notifyActiveCount()
        refreshApplicationSurfaces()
        val answer=CompletableFuture<String>()
        try{
            CompletableFuture.supplyAsync({db.configureInstance(id,version,spec)},io).whenComplete{value,failure->actor.post{
                if(dataOwners[id]==token)dataOwners.remove(id)
                refreshApplicationSurfaces()
                if(failure==null){
                    if(JSONObject(value).getLong("version").toULong()!=version)recentExits.remove(id)
                    answer.complete(value)
                }else answer.completeExceptionally(failure)
                finishCloseIfIdle()
            }}
        }catch(failure:Exception){dataOwners.remove(id);refreshApplicationSurfaces();answer.completeExceptionally(failure);finishCloseIfIdle()}
        answer
    }.thenCompose{it}
    fun groupNetworkPermissionReview(target:String):CompletableFuture<GroupNetworkPermissionReview> = submit{it.groupNetworkPermissionReview(target)}
    fun setGroupNetworkPermissions(review:GroupNetworkPermissionReview,internet:Boolean,lan:Boolean):CompletableFuture<GroupNetworkPermissionReview> = submit{db->
        val source=JSONObject(review.source());val id=source.getJSONObject("target").getString("id")
        check(id !in stackStarts&&id !in stackEdits){"Application definition or start is still active"}
        val members=source.getJSONArray("members")
        for(index in 0 until members.length()){
            val member=members.getJSONObject(index).getString("id")
            check(member !in jobs&&member !in dataOwners){"Stop all listed services and finish data operations before changing permissions"}
        }
        // Native validates the opaque all-member snapshot in one transaction.
        check(!lan || LocalNetworkPermission.granted(context)) { "Android local network permission is required" }
        val saved=db.setGroupNetworkPermissions(review,internet,lan)
        for(index in 0 until members.length())cancelAutomation(members.getJSONObject(index).getString("id"))
        refreshApplicationSurfaces();saved
    }
    fun networkPermissionSource(id:String):CompletableFuture<String> = submit{it.networkPermissionSource(id)}
    internal fun automaticStartSource(id: String): CompletableFuture<String> = submit { db ->
        JSONObject(db.automaticStartSource(id)).apply {
            if (id in jobs || id in dataOwners) put("editable", false)
        }.toString()
    }

    internal fun setAutomaticStart(
        id: String, identity: String, version: ULong, startOnBoot: Boolean, restartPolicy: String,
    ): CompletableFuture<Unit> = submit { db ->
        check(id !in jobs && id !in dataOwners) { "Stop this app and finish data operations" }
        // The native transaction validates identity and version before clearing
        // durable run intent. A stale draft never cancels the current app's work.
        val saved = db.setAutomaticStart(id, identity, version, startOnBoot, restartPolicy)
        cancelAutomation(id)
        if (saved != version) recentExits.remove(id)
        refreshApplicationSurfaces()
        notifyActiveCount()
    }
    fun setNetworkPermissions(id:String,identity:String,version:ULong,internet:Boolean,lan:Boolean):CompletableFuture<String> = submit{db->
        check(id !in jobs&&id !in dataOwners){"Stop this app and finish data operations before changing permissions"}
        val source=JSONObject(db.networkPermissionSource(id))
        check(source.getString("identity")==identity&&source.getLong("version").toULong()==version){"Application changed"}
        check(!source.getBoolean("managed")){"Managed application permissions require group review"}
        check(source.getString("state") in setOf("created","exited","dead")){"Stop this app before changing permissions"}
        check(!lan || LocalNetworkPermission.granted(context)) { "Android local network permission is required" }
        cancelAutomation(id);suppressDesiredRun(id)
        val token=UUID.randomUUID().toString();dataOwners[id]=token
        notifyActiveCount();refreshApplicationSurfaces()
        val answer=CompletableFuture<String>()
        try{
            CompletableFuture.supplyAsync({
                check(!lan || LocalNetworkPermission.granted(context)) { "Android local network permission is required" }
                db.setNetworkPermissions(id,identity,version,internet,lan)
            },io).whenComplete{value,failure->actor.post{
                if(dataOwners[id]==token)dataOwners.remove(id)
                refreshApplicationSurfaces()
                if(failure==null){
                    if(JSONObject(value).getLong("version").toULong()!=version)recentExits.remove(id)
                    answer.complete(value)
                }else answer.completeExceptionally(failure)
                finishCloseIfIdle()
            }}
        }catch(failure:Exception){dataOwners.remove(id);refreshApplicationSurfaces();answer.completeExceptionally(failure);finishCloseIfIdle()}
        answer
    }.thenCompose{it}
    fun remove(id: String,admission:(()->Unit)?=null): CompletableFuture<Unit> = submit {
        admission?.invoke()
        cancelAutomation(id)
        check(id !in dataOwners){"Data operation is still active"}
        check(!jobs.containsKey(id)) { "Stop service before removal" }
        suppressDesiredRun(id)
        val record = JSONObject(it.inspectInstance(id))
        it.removeInstance(id, record.getLong("version").toULong())
        refreshApplicationSurfaces()
        recentExits.remove(id)
        Unit
    }

    fun start(id: String,admission:(()->Unit)?=null): CompletableFuture<String> {
        val answer = CompletableFuture<String>()
        actor.post {
            try {
                check(!closing&&lifetime.admits()) { "Supervisor is closing or retiring" }
                admission?.invoke()
                cancelAutomation(id)
                check(id !in dataOwners){"Data operation is still active"}
                check(!jobs.containsKey(id)) { "Service already owns a live worker" }
                val record = JSONObject(storage.join().inspectInstance(id))
                val stackId=storage.join().instanceStack(id)
                if(stackId!=null){
                    startStack(stackId,id,admission).whenComplete{value,failure->if(failure==null)answer.complete(value)else answer.completeExceptionally(failure)}
                    return@post
                }
                val job=reserveStandalone(storage.join(),id,record)
                job.started.whenComplete { value, error -> if (error == null) answer.complete(value) else answer.completeExceptionally(error) }
            } catch (failure: Exception) { answer.completeExceptionally(failure) }
        }
        return answer
    }

    /** Actor-only worker admission shared by manual, restart and boot paths.
     * Reservation happens before any Binder side effect and before consuming an
     * automation ticket, so the foreground count cannot temporarily reach zero. */
    private fun reserveStandalone(db:EngineStorage,id:String,record:JSONObject,ticket:ServiceRestartCoordinator.Ticket?=null,source:String="manual"):Job{
        check(!closing&&lifetime.admits()&&id !in jobs&&id !in dataOwners){"Service is owned or supervisor is closing"}
        check(db.instanceStack(id)==null){"Managed service requires its stack controller"}
        check(record.getString("id")==id&&record.getString("state") in listOf("created","exited")){"Service cannot start from this state"}
        if(ticket!=null)check(restarts.valid(ticket,restartRecord(id))){"Automatic restart admission changed"}
        val current=JSONObject(db.inspectInstance(id))
        check(current.getLong("version")==record.getLong("version")){"Service revision changed"}
        val spec = current.getJSONObject("spec")
        db.validateServiceSpec(spec.toString())
        PackageRuntimeRequirements.validate(spec, JSONArray(db.listImages()))
        val slot=slots.indices.firstOrNull{index->jobs.values.none{it.slot==index}}
            ?:error("No free execution worker; stop a service first")
        val job=Job(id,slot,current.getLong("version").toULong(),current.getJSONObject("spec").toString())
        val authority=JSONObject(db.runAuthority(id))
        if(ticket!=null)check(authority.getLong("revision").toULong()==ticket.authorityRevision){"Run authority changed"}
        job.authority=JSONObject(db.beginAuthorizedRun(id,job.version,authority.getLong("revision").toULong(),job.generation,bootEpoch,source))
        try{job.egressRequired=db.workerEgressRequired(id);db.beginLogRun(id,job.generation)}catch(failure:Exception){
            // No Binder start has been sent; persist a terminal failed admission.
            runCatching{db.finishAuthorizedRun(id,job.version,job.generation,null,"Worker admission failed before launch",false)}
            throw failure
        }
        recentExits.remove(id);automaticFailures.remove(id)
        jobs[id]=job
        refreshApplicationSurfaces()
        restarts.started(id,job.generation,ticket)
        bind(job)
        actor.postDelayed({if(isCurrent(job)&&!job.started.isDone){
            job.error="Service startup timed out";job.started.completeExceptionally(IllegalStateException(job.error));stopJob(job,0,false)
        }},180_000)
        notifyActiveCount()
        return job
    }

    /** Reserve the whole requested dependency closure before binding any worker.
     * Caller-supplied labels never select members or construct network grants. */
    fun startStack(id:String,target:String?=null,admission:(()->Unit)?=null):CompletableFuture<String> {
        val result=CompletableFuture<String>()
        actor.post{
            val added=mutableListOf<Job>()
            try{
                check(!closing&&lifetime.admits()&&id !in stackStarts&&id !in stackEdits){"Stack operation is already active or supervisor is closing"}
                admission?.invoke()
                val db=storage.join();val definition=JSONObject(db.stackPlan(id)).getJSONObject("plan")
                val services=definition.getJSONObject("services");val members=JSONArray(db.stackMembers(id))
                val byName=(0 until members.length()).associate{index->val member=members.getJSONObject(index);member.getString("service") to member.getString("instance_id")}
                val wanted=mutableSetOf<String>()
                fun include(name:String){
                    if(!wanted.add(name))return
                    val dependencies=services.getJSONObject(name).getJSONArray("depends_on")
                    for(index in 0 until dependencies.length())include(dependencies.getString(index))
                }
                if(target==null)byName.keys.forEach{include(it)}
                else include(byName.entries.firstOrNull{it.value==target}?.key?:error("Service is not a stack member"))
                val startup=definition.getJSONArray("startup_order")
                val ordered=(0 until startup.length()).map{startup.getString(it)}.filter{it in wanted}
                val records=ordered.map{service->val instance=byName.getValue(service);instance to JSONObject(db.inspectInstance(instance))}
                records.forEach{(member,_)->cancelAutomation(member)}
                for((member,record) in records){
                    check(member !in dataOwners){"A stack data operation is still active"}
                    val current=jobs[member]
                    if(current!=null)check(current.stackId==id&&current.ready&&!current.stopping){"A stack service is still changing state"}
                    else check(record.getString("state") in listOf("created","exited")){"Retry the failed service before starting the stack"}
                }
                val free=slots.indices.filter{slot->jobs.values.none{it.slot==slot}}
                val needed=records.filter{(member,_)->member !in jobs}
                check(needed.size<=free.size){"Not enough free workers for this stack"}
                val installedImages by lazy { JSONArray(db.listImages()) }
                needed.forEach { (_, record) ->
                    PackageRuntimeRequirements.validate(record.getJSONObject("spec"), installedImages)
                }
                for((index,pair) in needed.withIndex()){
                    val(member,record)=pair
                    val job=Job(member,free[index],record.getLong("version").toULong(),record.getJSONObject("spec").toString(),id)
                    db.beginLogRun(member,job.generation);recentExits.remove(member);jobs[member]=job;added.add(job)
                }
                if(added.isNotEmpty()){
                    val targetJson=JSONObject(db.applicationTarget("group",id)).getJSONObject("target").toString()
                    val review=db.groupNetworkPermissionReview(targetJson)
                    val starts=JSONArray()
                    added.forEach{job->starts.put(JSONObject().put("instance_id",job.id).put("version",job.version.toLong()).put("generation",job.generation))}
                    added.forEach{it.managedAdmissionAttempted=true}
                    val authorities=JSONArray(db.beginManagedRuns(review,starts.toString()))
                    check(authorities.length()==added.size){"Incomplete managed run admission"}
                    // Assign every receipt before any later fallible lookup.
                    added.forEachIndexed{index,job->job.authority=authorities.getJSONObject(index)}
                    added.forEach{job->job.egressRequired=db.workerEgressRequired(job.id)}
                }
                val selected=jobs.values.filter{it.stackId==id&&!it.stopping}.toList()
                val operation=StackStart(id,selected,added.toList(),ordered.map{jobs.getValue(byName.getValue(it))}.filter{it in added},result)
                stackStarts[id]=operation;refreshApplicationSurfaces();notifyActiveCount()
                configureStackNetwork(id,selected).whenComplete{_,failure->actor.post{
                    if(failure!=null||operation.cancelled)rollbackStack(operation,failure?:IllegalStateException("Stack start cancelled"))
                    else launchStackMember(operation,0)
                }}
            }catch(failure:Exception){
                if(stackStarts[id]?.result===result)stackStarts.remove(id)
                added.forEach{releaseUnbound(it,failure)};notifyActiveCount();result.completeExceptionally(failure)
            }
        }
        return result
    }
    private fun configureStackNetwork(id:String,members:List<Job>):CompletableFuture<Unit> {
        if(members.isEmpty())return CompletableFuture.completedFuture(Unit)
        return try{
            check(networkRevision<Long.MAX_VALUE.toULong()){"Network revision exhausted"};networkRevision++
            val revision=networkRevision;val workers=JSONObject();val previous=JSONObject()
            members.forEach{job->workers.put(job.id,job.generation);job.networkConfig?.let{previous.put(job.id,JSONObject(it))}}
            val configurations=JSONObject(storage.join().stackNetworkConfigs(id,workers.toString(),previous.toString(),revision))
            val updates=members.map{job->
                val config=configurations.getJSONObject(job.id).toString()
                if(job.ready){
                    terminalCommand(job.id,job.generation,WorkerProtocol.NETWORK,Bundle().apply{putString(WorkerProtocol.NETWORK_CONFIG,config)}).thenApply{
                        if(isCurrent(job)&&revision>=job.networkVersion){job.networkConfig=config;job.networkVersion=revision};Unit
                    }
                }else{
                    check(!job.bindRequested){"Starting worker cannot change its initial topology"}
                    job.networkConfig=config;job.networkVersion=revision;CompletableFuture.completedFuture(Unit)
                }
            }
            CompletableFuture.allOf(*updates.toTypedArray()).thenApply{Unit}
        }catch(failure:Exception){CompletableFuture<Unit>().apply{completeExceptionally(failure)}}
    }
    private fun launchStackMember(operation:StackStart,index:Int){
        if(operation.cancelled||closing||!lifetime.admits()){rollbackStack(operation,IllegalStateException("Stack start cancelled"));return}
        if(index==operation.order.size){
            val live=jobs.values.filter{it.stackId==operation.id&&it.ready&&!it.stopping}.toList()
            configureStackNetwork(operation.id,live).whenComplete{_,failure->actor.post{
                if(failure!=null||operation.cancelled)rollbackStack(operation,failure?:IllegalStateException("Stack start cancelled"))
                else{
                    stackStarts.remove(operation.id)
                    try{operation.result.complete(JSONObject().put("stack_id",operation.id).put("members",JSONArray(storage.join().stackMembers(operation.id))).toString())}
                    catch(error:Exception){operation.result.completeExceptionally(error)}
                    finishCloseIfIdle()
                }
            }}
            return
        }
        val job=operation.order[index]
        job.started.whenComplete{_,failure->actor.post{
            if(failure!=null)rollbackStack(operation,failure)else launchStackMember(operation,index+1)
        }}
        bind(job)
        actor.postDelayed({if(isCurrent(job)&&!job.started.isDone){
            job.error="Service startup timed out";job.started.completeExceptionally(IllegalStateException(job.error));stopJob(job,0,false)
        }},180_000)
    }
    private fun releaseUnbound(job:Job,failure:Throwable){
        if(!isCurrent(job)||job.bindRequested)return
        if(job.authority!=null||job.managedAdmissionAttempted){
            try{
                if(job.stackId!=null){
                    val db=storage.join();val actual=JSONObject(db.runAuthority(job.id))
                    if(actual.optString("generation")==job.generation){
                        job.authority=JSONObject(db.withdrawUnboundManagedRun(job.id,job.version,job.generation))
                    }else{
                        check(actual.getString("outcome") !in setOf("starting","running")){"Managed admission receipt changed"}
                    }
                    job.managedAdmissionAttempted=false
                }else{
                    val acknowledgement=JSONObject(storage.join().finishAuthorizedRun(job.id,job.version,job.generation,null,"Worker admission ended before binding",false))
                    job.authority=acknowledgement.getJSONObject("authority")
                }
            }catch(persist:Exception){
                job.started.completeExceptionally(persist)
                actor.postDelayed({releaseUnbound(job,failure)},1_000)
                return
            }
        }
        jobs.remove(job.id);job.started.completeExceptionally(failure)
        try{job.ended.complete(JSONObject().put("cancelled",true).put("instance",JSONObject(storage.join().inspectInstance(job.id)))
            .put("generation",job.generation).put("guest_result",JSONObject.NULL).toString())}
        catch(error:Exception){job.ended.completeExceptionally(error)}
        refreshApplicationSurfaces();finishCloseIfIdle()
    }
    private fun rollbackStack(operation:StackStart,failure:Throwable){
        if(operation.rollingBack||operation.retryingFence)return
        operation.cancelled=true
        // A failed durable fence retains all jobs and the operation, keeps this
        // actor alive, and prevents launch/slot release until a retry commits.
        try{
            operation.added.filter(::isCurrent).filter{it.authority!=null}.forEach{suppressDesiredRun(it.id,it.generation)}
        }catch(_:Exception){
            operation.retryingFence=true
            actor.postDelayed({operation.retryingFence=false;rollbackStack(operation,failure)},1_000)
            return
        }
        operation.rollingBack=true
        operation.added.asReversed().forEach{job->if(isCurrent(job)){
            if(!job.bindRequested)releaseUnbound(job,failure)
            // The group failure belongs to operation.result. Preserve each
            // worker's verified exit/error instead of inventing a guest error.
            else stopJob(job,0,false)
        }}
        notifyActiveCount()
        CompletableFuture.allOf(*operation.added.map{it.ended}.toTypedArray()).whenComplete{_,cleanupFailure->actor.post{
            val remaining=jobs.values.filter{it.stackId==operation.id&&it.ready&&!it.stopping}.toList()
            configureStackNetwork(operation.id,remaining).whenComplete{_,routeFailure->actor.post{
                stackStarts.remove(operation.id)
                operation.result.completeExceptionally(cleanupFailure?:routeFailure?:failure);finishCloseIfIdle()
            }}
        }}
    }
    fun stopStack(id:String,admission:(()->Unit)?=null):CompletableFuture<Unit>{
        val result=CompletableFuture<Unit>();actor.post{
            try{
                check(id !in stackEdits){"Stack definition update is still active"}
                admission?.invoke()
                val members=JSONArray(storage.join().stackMembers(id))
                for(index in 0 until members.length()){
                    val member=members.getJSONObject(index).getString("instance_id")
                    suppressDesiredRun(member,jobs[member]?.generation);cancelAutomation(member)
                }
                stackStarts[id]?.let{operation->operation.cancelled=true;rollbackStack(operation,IllegalStateException("Stack stopped during startup"))}
                var sequence=stackStarts[id]?.result?.handle{_,_->Unit}?:CompletableFuture.completedFuture(Unit)
                for(index in members.length()-1 downTo 0){
                    val member=members.getJSONObject(index).getString("instance_id")
                    sequence=sequence.thenCompose{stop(member).thenApply{Unit}}
                }
                sequence.whenComplete{_,failure->if(failure==null)result.complete(Unit)else result.completeExceptionally(failure)}
            }catch(failure:Exception){result.completeExceptionally(failure)}
        };return result
    }
    private fun bind(job: Job) {
        if(!isCurrent(job)||job.stopping||closing||!lifetime.admits()){job.started.completeExceptionally(IllegalStateException("Service start cancelled"));return}
        job.bindRequested=true
        val connection = object : ServiceConnection {
            override fun onServiceConnected(name: ComponentName?, service: IBinder) { actor.post { connected(job, service) } }
            override fun onServiceDisconnected(name: ComponentName?) { actor.post { died(job) } }
            override fun onBindingDied(name: ComponentName?) { actor.post { died(job) } }
            override fun onNullBinding(name: ComponentName?) { actor.post {
                if (isCurrent(job)) { job.error = "Worker binding rejected"; died(job) }
            } }
        }
        job.connection = connection
        try {
            job.bound = context.bindService(Intent(context, slots[job.slot]), connection, Context.BIND_AUTO_CREATE)
            check(job.bound) { "Worker service unavailable" }
        } catch (failure: Exception) { job.error = failure.message; died(job) }
    }
    private fun connected(job: Job, binder: IBinder) {
        if (!isCurrent(job)) return
        job.binder = binder
        job.messenger = Messenger(binder)
        try {
            binder.linkToDeath({ actor.post { died(job) } }, 0)
            val owner = ParcelFileDescriptor.adoptFd(storage.join().duplicateWorkerOwner())
            owner.use {
                send(job, WorkerProtocol.START, Bundle().apply {
                    putString(WorkerProtocol.SPEC, job.spec)
                    putString(WorkerProtocol.INSTANCE, job.id)
                    putParcelable(WorkerProtocol.OWNER, owner)
                    putBoolean(WorkerProtocol.STOP_BEFORE_START, job.stopping)
                    putBoolean(WorkerProtocol.EGRESS_REQUIRED,job.egressRequired)
                    job.networkConfig?.let{putString(WorkerProtocol.NETWORK_CONFIG,it)}
                })
            }
            if (job.stopping) stopJob(job, 0, job.stopRequested)
        } catch (failure: Exception) { job.error = failure.message; stopJob(job, 0, false) }
    }
    private val replies = Messenger(Handler(thread.looper) { message ->
        if (message.sendingUid != Process.myUid()) return@Handler true
        val token = message.data.getString(WorkerProtocol.GENERATION)
        val job = jobs.values.firstOrNull { it.generation == token } ?: return@Handler true
        if (message.data.getInt(WorkerProtocol.UID) != Process.myUid() ||
            message.data.getInt(WorkerProtocol.PID) == Process.myPid()) return@Handler true
        val result = message.data.getString(WorkerProtocol.RESULT) ?: ""
        try {
            when (message.what) {
                WorkerProtocol.READY -> {
                    if (job.ready) return@Handler true
                    val info = JSONObject(result)
                    job.ports = info.getJSONArray("ports")
                    job.runtimeInfo = info
                    val record = if(job.authority!=null){
                        val acknowledgement=JSONObject(storage.join().readyAuthorizedRun(job.id,job.version,job.generation,job.ports.toString()))
                        job.authority=acknowledgement.getJSONObject("authority")
                        acknowledgement.getJSONObject("instance")
                    }else JSONObject(storage.join().markRunning(job.id, job.version, job.ports.toString()))
                    job.version = record.getLong("version").toULong()
                    job.ready = true
                    notifyActiveCount()
                    refreshApplicationSurfaces()
                    job.readyAtMillis=SystemClock.elapsedRealtime()
                    if(!job.stopping)browserSessions.publish(job.id,job.generation,job.ports.toString(),storage.join().browserIdentity(job.id))
                    job.started.complete(snapshot(job))
                    if (job.stackId == null) {
                        runCatching { sharingEndpoint(job.id)?.let(lanSharing::readyDefault) }
                    }
                    val timeout = JSONObject(job.spec).optJSONObject("resources")?.optLong("timeout_seconds", 0) ?: 0
                    if (timeout > 0 && !job.stopping) {
                        val now = SystemClock.uptimeMillis()
                        val duration = timeout.coerceAtMost(Long.MAX_VALUE / 1_000) * 1_000
                        val deadline = now + duration.coerceAtMost(Long.MAX_VALUE - now)
                        actor.postAtTime({
                            if (isCurrent(job) && !job.stopping) {
                                job.error = "Service execution timed out"
                                stopJob(job, 5_000, false)
                            }
                        }, job, deadline)
                    }
                }
                WorkerProtocol.COMPLETED -> {
                    job.result = result
                    job.exit = JSONObject(result).getInt("exit_code")
                    job.receivedStopBeforeExit = message.data.getBoolean(WorkerProtocol.STOP_RECEIVED)
                    storage.join().saveOutput(job.id, job.generation, result)
                    send(job, WorkerProtocol.COMPLETE_ACK)
                }
                WorkerProtocol.FAILED -> {
                    job.error = result
                    send(job, WorkerProtocol.COMPLETE_ACK)
                }
                WorkerProtocol.CONTROL_DONE,WorkerProtocol.CONTROL_ERROR -> {
                    val request=message.data.getString(WorkerProtocol.REQUEST)?:return@Handler true
                    val command=job.commands.remove(request)?:return@Handler true
                    if(job.stdinCommand==request)job.stdinCommand=null
                    if(message.what==WorkerProtocol.CONTROL_DONE)command.complete(result)
                    else command.completeExceptionally(IllegalStateException(result))
                }
            }
        } catch (failure: Exception) { job.error = failure.message; stopJob(job, 0, false) }
        true
    })
    private fun send(job: Job, what: Int, extras: Bundle = Bundle()) {
        extras.putString(WorkerProtocol.GENERATION, job.generation)
        val message = Message.obtain(null, what).apply { data = extras; replyTo = replies }
        job.messenger?.send(message)
    }
    fun stop(id: String, graceMillis: Long = 5_000,admission:(()->Unit)?=null): CompletableFuture<String> {
        val answer = CompletableFuture<String>()
        actor.post {
            try {
                admission?.invoke()
                suppressDesiredRun(id,jobs[id]?.generation)
                cancelAutomation(id)
                val job = jobs[id]
                if (job == null) {refreshApplicationSurfaces();answer.complete(storage.join().inspectInstance(id))}
                else {
                    job.ended.whenComplete { value, failure -> if (failure == null) answer.complete(value) else answer.completeExceptionally(failure) }
                    stopJob(job,graceMillis.coerceIn(0,60_000),true,true)
                }
            } catch (failure: Exception) { answer.completeExceptionally(failure) }
        }
        return answer
    }
    private fun stopJob(job: Job, graceMillis: Long, ownerStop: Boolean = true,intentRecorded:Boolean=false) {
        if (!isCurrent(job)) return
        if(ownerStop){
            if(!intentRecorded)suppressDesiredRun(job.id,job.generation)
            cancelAutomation(job.id)
        }
        browserSessions.revoke(job.id,job.generation)
        val operation=job.stackId?.let{stackStarts[it]}
        if(operation!=null&&!operation.rollingBack){
            rollbackStack(operation,IllegalStateException("Stack stopped during startup"))
            if(!isCurrent(job))return
            if(job in operation.added&&job.stopping){
                job.stopRequested=job.stopRequested||ownerStop;return
            }
            // Rollback stops only newly reserved members. A requested stop of
            // an already running dependency must still reach that worker.
        }
        if(!job.bindRequested){releaseUnbound(job,IllegalStateException("Service start cancelled"));notifyActiveCount();return}
        job.stopping = true
        actor.removeCallbacksAndMessages(job)
        lanSharing.stopping(job.id,job.generation)
        notifyActiveCount()
        refreshApplicationSurfaces()
        job.stopRequested = job.stopRequested || ownerStop
        runCatching { send(job, WorkerProtocol.STOP,Bundle().apply {
            putLong(WorkerProtocol.GRACE_SECONDS,(graceMillis.coerceIn(0,60_000)+999)/1000)
        }) }
        actor.postDelayed({
            if (isCurrent(job)) {
                job.forced = true
                // Ask only this live Binder endpoint to kill its own process;
                // never kill an integer PID that could have been reused.
                runCatching { send(job, WorkerProtocol.TERMINATE) }
                actor.postDelayed({
                    if (isCurrent(job)) {
                        job.ended.completeExceptionally(IllegalStateException("Worker death was not observed; service still owns resources"))
                    }
                }, 5_000)
            }
        }, graceMillis)
    }
    private fun isCurrent(job: Job): Boolean = jobs[job.id] === job
    private fun snapshot(job: Job): String = JSONObject().apply {
        put("id", job.id); put("generation", job.generation); put("ports", job.ports)
        put("stopping", job.stopping); put("forced", job.forced)
        put("runtime_info",job.runtimeInfo)
    }.toString()
    private fun died(job: Job) {
        if (!isCurrent(job)) return
        // A disconnected callback is not enough if the old Binder still lives.
        if (job.binder?.isBinderAlive == true) return
        actor.removeCallbacksAndMessages(job)
        var record: String? = null
        try {
            val db = storage.join()
            val reason = workerExitReason(job.exit, job.error, job.stopRequested, job.receivedStopBeforeExit)
            record = if(job.authority!=null){
                val acknowledgement=JSONObject(db.finishAuthorizedRun(job.id,job.version,job.generation,job.exit,
                    reason,job.stopRequested))
                job.authority=acknowledgement.getJSONObject("authority")
                acknowledgement.getJSONObject("instance").toString()
            }else if (job.stopRequested && job.exit == null)db.markStopped(job.id,job.version)
            else db.markFinished(job.id,job.version,job.exit,reason)
        } catch (failure: Exception) {
            // Do not release the slot or report persisted stopped state until
            // the terminal state/event transaction has actually committed.
            job.ended.completeExceptionally(failure)
            actor.postDelayed({ died(job) }, 1_000)
            return
        }
        browserSessions.revoke(job.id,job.generation)
        job.connection?.let { if (job.bound) runCatching { context.unbindService(it) } }
        job.commands.values.forEach{it.completeExceptionally(IllegalStateException("Terminal generation ended"))};job.commands.clear();job.stdinCommand=null
        jobs.remove(job.id)
        lanSharing.ended(job.id,job.generation)
        if(job.stackId==null){
            try{
                // The committed reply is the terminal authority. Further DB
                // reads belong to restart admission, never receipt cleanup.
                val committed=JSONObject(requireNotNull(record))
                val terminal=authorityRecord(job.id,committed,requireNotNull(job.authority))
                restarts.completed(ServiceRestartCoordinator.Completion(
                    terminal,job.generation,if(committed.isNull("exit_code"))null else committed.getInt("exit_code"),
                    !committed.isNull("last_error"),job.stopRequested||closing,true,true,
                    job.readyAtMillis?.let{(SystemClock.elapsedRealtime()-it).coerceAtLeast(0)}?:0,
                ))
            }catch(failure:Exception){recordAutomaticFailure(job.id,"restart_completion_failed")}
        }
        if (!job.started.isDone) job.started.completeExceptionally(IllegalStateException(job.error ?: "Service ended during startup"))
        val receipt=JSONObject().apply {
            put("instance", record?.let(::JSONObject)); put("forced", job.forced)
            put("generation",job.generation)
            put("guest_result", job.result?.let(::JSONObject)?:JSONObject.NULL)
        }.toString()
        recentExits[job.id]=receipt
        refreshApplicationSurfaces()
        while(recentExits.size>16)recentExits.remove(recentExits.keys.first())
        if (!job.ended.isDone) job.ended.complete(receipt)
        job.stackId?.let{id->
            val operation=stackStarts[id]
            if(operation!=null&&operation.members.contains(job)&&!operation.rollingBack&&(job.exit!=0||job.stopRequested))
                rollbackStack(operation,IllegalStateException("A stack service ended during startup"))
            else if(operation==null){
                val remaining=jobs.values.filter{it.stackId==id&&it.ready&&!it.stopping}.toList()
                configureStackNetwork(id,remaining).whenComplete{_,failure->if(failure!=null)actor.post{
                    // Preserve other processes; a failed update never grants new
                    // peers. Existing dead transport endpoints remain closed.
                    remaining.filter{isCurrent(it)&&it.ready&&!it.stopping}.forEach{it.runtimeInfo.put("network_warning","Private routing update failed")}
                }}
            }
        }
        notifyActiveCount()
        finishCloseIfIdle()
    }
    fun stopAll(): CompletableFuture<Unit> {
        val answer = CompletableFuture<Unit>()
        actor.post {
            try{
                storage.join().stopAllRunIntents()
                refreshApplicationSurfaces()
                cancelAllAutomation()
                val live=jobs.values.toList()
                live.forEach{stopJob(it,5_000,true,true)}
                CompletableFuture.allOf(*live.map{it.ended}.toTypedArray()).whenComplete{_,failure->
                    if(failure==null)answer.complete(Unit)else answer.completeExceptionally(failure)
                }
            }catch(failure:Exception){answer.completeExceptionally(failure)}
        }
        return answer
    }
    fun close(): CompletableFuture<Unit> {
        lifetime.destroy()
        actor.post {
            closing = true
            lanSharing.close()
            browserSessions.close()
            cancelAllAutomation()
            catalogUpdates.close()
            packageOperations.values.forEach{runCatching{it.control.cancel()}}
            packageOperations.values.filter{it.kind=="local_import"}.forEach{localPackages.cancelled(it.key,it.generation)}
            // Owner destruction is not a user Stop. Persisted desired-run
            // authority remains available to a later same-boot owner.
            jobs.values.toList().forEach { stopJob(it,0,false) }
            finishCloseIfIdle()
        }
        return closeFuture
    }
    private fun finishCloseIfIdle() {
        // The last admin/catalog/package reservation may finish after the last
        // guest. Recheck idle retirement even when the owner is staying open.
        if(!closing)notifyActiveCount()
        if (closing && jobs.isEmpty() && restarts.pendingCount==0 && pendingBoot.isEmpty() && bootRun==null && recoveryScan==null && lifetime.backgroundCount()==0 && dataOwners.isEmpty() && stackStarts.isEmpty() && stackEdits.isEmpty() && packageOperations.isEmpty() && catalogUpdates.idle && !storageClosed) {
            storageClosed = true
            recentExits.clear()
            recentPackages.clear()
            localPackages.shutdown()
            // Registry work retains native ownership until it completes.
            storage.whenComplete { value, _ -> value?.close() }
            io.shutdown()
            logIo.shutdown()
            closeFuture.complete(Unit)
            thread.quitSafely()
        }
    }
}
