package dev.wasmdroid.app.ui

import android.app.Application
import android.content.*
import android.os.IBinder
import androidx.core.content.edit
import androidx.lifecycle.AndroidViewModel
import dev.wasmdroid.app.engine.EngineService
import dev.wasmdroid.app.engine.ServiceSupervisor
import dev.wasmdroid.app.engine.PackageActionFailure
import dev.wasmdroid.app.engine.availableTransferBytes
import dev.wasmdroid.engine.engineInfo
import dev.wasmdroid.app.reports.ReportOperation
import kotlinx.coroutines.*
import kotlinx.coroutines.flow.*
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock
import org.json.JSONArray
import org.json.JSONObject
import java.util.UUID
import java.util.concurrent.CompletableFuture
import java.util.concurrent.CompletionException
import dev.wasmdroid.engine.DataTransferControl

data class AppState(
    val connected: Boolean = false, val runtimeAvailable: Boolean = false,
    val advanced: Boolean = false,
    val instances: List<String> = emptyList(), val images: List<String> = emptyList(),
    val volumes: List<String> = emptyList(), val registry: List<String> = emptyList(),
    val ports: Map<String, String> = emptyMap(), val busy: Set<String> = emptySet(),
    val progress: String? = null, val error: String? = null, val catalogError: Boolean = false,
    val errorKind:String?=null,val errorOperation:String?=null,val errorStage:String?=null,
    val sharingTarget:String?=null,
    val controllers: Map<String,String> = emptyMap(),
    val groups:List<String> = emptyList(), val moreGroups:Boolean=false,
    val applications:List<String> = emptyList(),
    internal val applicationFocus:String?=null,internal val shortcutWeb:WebTarget?=null,
    val memberships:Map<String,String> = emptyMap(),
    val dataRevision:Long=0,val notice:Int?=null,
    val packages:List<PackageTask> = emptyList(),val packageCancels:Set<String> = emptySet(),val packagesUnavailable:Boolean=false,
    internal val localPackageSelection:LocalPackageSelection?=null,internal val localPackage:LocalPackageView?=null,
    val localPackageUnavailable:Boolean=false,val registryQuery:String="",
    internal val registrySearch:RegistrySearchState=RegistrySearchState(),
    internal val storageUsage:StorageUsageState=StorageUsageState(),
    val catalogUpdateConfiguration:String="disabled",val catalogRemote:Boolean=false,
    val catalogFreshness:String?=null,val catalogRefreshing:Boolean=false,
    val catalogRefreshGeneration:String?=null,val catalogRefreshOutcome:String?=null,
    val browserEndpoints:Map<String,String> = emptyMap(),
)
class AppModel(application: Application) : AndroidViewModel(application) {
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.Main.immediate)
    private val preferences = application.getSharedPreferences("settings", 0)
    private val mutable = MutableStateFlow(AppState(advanced = preferences.getBoolean("advanced", false)))
    val state: StateFlow<AppState> = mutable.asStateFlow()
    private var ready = CompletableDeferred<ServiceSupervisor>()
    private var connectedEngine:ServiceSupervisor?=null
    internal val automaticStart = AutomaticStartController(scope, {
        val connection = ready
        val engine = connection.await()
        object : AutomaticStartBackend {
            override val current get() = connection === ready && engine === connectedEngine && mutable.value.connected
            override suspend fun source(id: String) = AutomaticStartSource.parse(engine.automaticStartSource(id).await())
            override suspend fun save(source: AutomaticStartSource, startOnBoot: Boolean, restartPolicy: AutomaticRestartPolicy) {
                engine.setAutomaticStart(source.id, source.identity, source.version, startOnBoot, restartPolicy.wire).await()
            }
        }
    }, { owner -> if (owner.current) refresh(ready.await()) })
    internal val networkPermissions=NetworkPermissionController(scope,{
        val captured=ready;val engine=captured.await()
        object:NetworkPermissionBackend{
            override val current:Boolean get()=captured===ready&&engine===connectedEngine&&mutable.value.connected
            override suspend fun source(id:String)=NetworkPermissionCodec.parse(engine.networkPermissionSource(id).await())
            override suspend fun save(source:NetworkPermissionSource,internet:Boolean,lan:Boolean){
                engine.setNetworkPermissions(source.id,source.identity,source.version,internet,lan).await()
            }
        }
    },{owner->if(owner.current){refresh(ready.await());mutable.update{it.copy(notice=dev.wasmdroid.app.R.string.network_permission_saved)}}})
    internal val groupNetworkPermissions=GroupNetworkPermissionController(scope,{
        val captured=ready;val engine=captured.await()
        object:GroupPermissionBackend{
            override val current:Boolean get()=captured===ready&&engine===connectedEngine&&mutable.value.connected
            override suspend fun review(target:String):GroupPermissionReview{
                val native=engine.groupNetworkPermissionReview(target).await()
                return NativeGroupPermissionReview(native,GroupNetworkPermissionCodec.parse(native.source()))
            }
            override suspend fun save(review:GroupPermissionReview,internet:Boolean,lan:Boolean){
                val actual=review as NativeGroupPermissionReview
                engine.setGroupNetworkPermissions(actual.native,internet,lan).await()
            }
        }
    },{owner->if(owner.current){refresh(ready.await());mutable.update{it.copy(notice=dev.wasmdroid.app.R.string.network_permission_saved)}}})
    internal val storageUsage = StorageUsageController(scope, {
        val captured = ready
        val engine = captured.await()
        object : StorageUsageBackend {
            override val current: Boolean get() = captured === ready && mutable.value.connected
            override suspend fun snapshot() = StorageUsageCodec.parse(engine.storageUsage().await())
        }
    })
    internal val registrySearch=RegistrySearchController(scope,{
        val captured=ready
        val engine=captured.await()
        object:RegistrySearchBackend {
            override val current:Boolean get()=captured===ready&&mutable.value.connected
            override suspend fun search(query:String,cursor:String?):RegistrySearchPage {
                val exactRelease = if (cursor == null) testedRegistryMatch(query) else null
                val raw=engine.search(query,cursor,exactRelease).await()
                require(raw.toByteArray(Charsets.UTF_8).size<=RegistrySearchController.MAX_PAGE_BYTES){"Registry search page exceeds limit"}
                val page=JSONObject(raw)
                require(page.has("next_cursor")){"Registry search cursor is missing"}
                val rows=page.getJSONArray("packages")
                require(rows.length()<=RegistrySearchController.MAX_PAGE_ROWS){"Registry search page exceeds row limit"}
                val next=if(page.isNull("next_cursor"))null else page.get("next_cursor").let{
                    require(it is String){"Invalid registry search cursor"};it
                }
                return RegistrySearchPage((0 until rows.length()).map{index->
                    val row=rows.getJSONObject(index)
                    val reference=row.get("reference");require(reference is String){"Invalid registry package reference"}
                    RegistrySearchRow(reference,row.toString(),parseRegistryReleaseDate(row.opt("released_at")),row.opt("webc_version") as? Int)
                },next)
            }
        }
    },{failure->fail(failure,ReportOperation.Search)})
    internal val configuration=ServiceConfigurationController(scope,{
        val captured=ready
        val engine=captured.await()
        object:ConfigurationBackend{
            override val current:Boolean get()=captured===ready&&mutable.value.connected
            override suspend fun inspect(id:String)=engine.inspect(id).await()
            override suspend fun group(id:String)=JSONObject(engine.memberships(listOf(id)).await()).optString(id).takeIf{it.isNotEmpty()}
            override suspend fun create(spec:String)=engine.create("app-${UUID.randomUUID()}",spec).await()
            override suspend fun configure(id:String,version:ULong,spec:String)=engine.configure(id,version,spec).await()
            override suspend fun createVolume(name:String)=engine.createNewVolume(name).await()
        }
    },{backend->if(backend.current)refresh(ready.await())})
    internal val stackImport=StackImportController(application,scope,object:StackImportBackend{
        override suspend fun requirements(document:String)=ready.await().stackPackageRequirements(document).await()
        override suspend fun preview(id:String,document:String)=ready.await().previewStack(id,document).await()
        override suspend fun create(id:String,document:String,preview:String){ready.await().createReviewedStack(id,preview).await()}
    },{
        try{refresh(ready.await())}catch(failure:Exception){if(failure is CancellationException)throw failure;fail(failure)}
    })
    internal val stackEdit=StackEditController(scope,{
        val captured=ready
        val engine=captured.await()
        object:StackEditBackend{
            override val current:Boolean get()=captured===ready&&mutable.value.connected
            override suspend fun source(id:String)=engine.stackEditSource(id).await()
            override suspend fun requirements(document:String)=engine.stackPackageRequirements(document).await()
            override suspend fun preview(id:String,version:ULong,document:String)=engine.previewStackUpdate(id,version,document).await()
            override suspend fun apply(id:String,preview:String)=engine.updateReviewedStack(id,preview).await()
        }
    },{backend->if(backend.current)refresh(ready.await())})
    private var groupPages=1
    private val refreshMutex=Mutex()
    private val packageMutex=Mutex()
    private var bound = false
    private val uiVisible=java.util.concurrent.atomic.AtomicBoolean(false)
    internal fun uiVisible(value:Boolean){uiVisible.set(value)}
    private fun sharingAdmission():Boolean = uiVisible.get()&&runCatching{
        val keyguard=getApplication<Application>().getSystemService(android.app.KeyguardManager::class.java)
        !keyguard.isDeviceLocked&&!keyguard.isKeyguardLocked
    }.getOrDefault(false)
    private val dataOperations=mutableMapOf<String,DataOperation>()
    private val connection = object : ServiceConnection {
        override fun onServiceConnected(name: ComponentName?, binder: IBinder?) {
            val engine = (binder as? EngineService.LocalBinder)?.engine ?: return
            connectedEngine=engine
            mutable.update { it.copy(connected = true) }
            if (!ready.isCompleted) ready.complete(engine)
            registrySearch.availability(true)
            storageUsage.availability(true)
        }
        override fun onServiceDisconnected(name: ComponentName?) {
            connectedEngine=null
            ready = CompletableDeferred()
            registrySearch.availability(false)
            storageUsage.availability(false)
            mutable.update { it.copy(connected = false,
                error = "Engine disconnected",errorKind="other",errorOperation="unknown",errorStage="failed") }
        }
    }
    init {
        registrySearch.availability(false)
        storageUsage.availability(false)
        scope.launch { storageUsage.state.collect { usage ->
            mutable.update { it.copy(storageUsage = usage) }
        } }
        scope.launch { registrySearch.state.collect{search->mutable.update{it.copy(
            registrySearch=search,registryQuery=search.draftQuery,registry=search.rows.map{row->row.json},
            busy=if(search.loading)it.busy+"search" else it.busy-"search",
        )}} }
        try {
            bound = application.bindService(Intent(application, EngineService::class.java), connection, Context.BIND_AUTO_CREATE)
            if (!bound) ready.completeExceptionally(IllegalStateException("Engine service unavailable"))
        } catch (failure: Exception) { ready.completeExceptionally(failure) }
        scope.launch {
            while(isActive){
                try{refreshPackages()}
                catch(failure:Exception){if(failure is CancellationException)throw failure;mutable.update{it.copy(packagesUnavailable=true)}}
                delay(500)
            }
        }
        scope.launch {
            runCatching { withContext(Dispatchers.IO) { engineInfo().runtimeProbeAvailable } }
                .onSuccess { available -> mutable.update { it.copy(runtimeAvailable = available) } }
            try {
                while (isActive) {
                    try { refresh(ready.await()) }
                    catch (failure: Exception) { if(failure is CancellationException)throw failure;fail(failure) }
                    delay(1_500)
                }
            } catch (failure: Exception) {
                if (failure is CancellationException) throw failure
                fail(failure)
            }
        }
    }
    private suspend fun refresh(engine: ServiceSupervisor) = refreshMutex.withLock {
        val captured=ready
        if(engine!==connectedEngine||!mutable.value.connected)return@withLock
        val instances = array(JSONArray(engine.instances().await()))
        val images = array(JSONArray(engine.images().await()))
        val volumes = array(JSONArray(engine.volumes().await()))
        val ports = mutableMapOf<String, String>()
        val browserEndpoints=mutableMapOf<String,String>()
        for (json in instances) {
            val record = JSONObject(json)
            if (record.getString("state") == "running") {
                val id = record.getString("id")
                val endpoint=engine.browserEndpoint(id).await()
                if(endpoint!="null"){
                    browserEndpoints[id]=endpoint
                    ports[id]=JSONObject(endpoint).getJSONArray("ports").toString()
                }
            }
        }
        val rawControllers=JSONObject(engine.controllers().await())
        val controllers=rawControllers.keys().asSequence().associateWith{rawControllers.getString(it)}
        val membershipJson=JSONObject(engine.memberships(instances.map{JSONObject(it).getString("id")}).await())
        val memberships=membershipJson.keys().asSequence().associateWith{membershipJson.getString(it)}
        val groups=mutableListOf<String>();var after:String?=null;var more=false
        for(pageIndex in 0 until groupPages){
            val page=array(JSONArray(engine.stacks(after,100u).await()))
            groups.addAll(page);more=page.size==100
            if(!more)break
            after=JSONObject(page.last()).getString("id")
        }
        val applications=mutableListOf<String>()
        for(group in groups){applications.add(engine.applicationTarget("group",JSONObject(group).getString("id")).await())}
        mutable.value.applicationFocus?.let{target->
            try{
                val focused=engine.shortcutResolve(target).await()
                if(JSONObject(focused).getJSONObject("target").getString("kind")=="group"){
                    val identity=JSONObject(focused).getJSONObject("target").getString("identity_token")
                    applications.removeAll{JSONObject(it).getJSONObject("target").getString("identity_token")==identity}
                    applications.add(0,focused)
                }
            }catch(failure:Exception){if(failure is CancellationException)throw failure
                if(captured===ready&&engine===connectedEngine)mutable.update{it.copy(applicationFocus=null)}}
        }
        if(captured!==ready||engine!==connectedEngine)return@withLock
        mutable.update { it.copy(instances = instances, images = images, volumes = volumes, ports = ports,controllers=controllers,
            groups=groups,moreGroups=more,memberships=memberships,browserEndpoints=browserEndpoints,applications=applications) }
    }
    private fun perform(key: String, refreshOnFailure:Boolean=false, reportOperation:ReportOperation=ReportOperation.Unknown, action: suspend (ServiceSupervisor) -> Unit) {
        if (key in mutable.value.busy) return
        mutable.update { it.copy(busy = it.busy + key, error = null,errorKind=null,errorOperation=null,errorStage=null) }
        scope.launch {
            try { val engine = ready.await(); action(engine); refresh(engine) }
            catch (failure: Exception) {
                if (failure is CancellationException) throw failure
                fail(failure,reportOperation)
                if(refreshOnFailure)runCatching{refresh(ready.await())}
            }
            finally { mutable.update { it.copy(busy = it.busy - key) } }
        }
    }
    private suspend fun refreshPackages()=packageMutex.withLock{
        val current=ready
        val engine=current.await()
        val rows=packageTasks(engine.packageActivity().await())
        if(current===ready)mutable.update{state->state.copy(packages=rows,packagesUnavailable=false,
            packageCancels=state.packageCancels.intersect(rows.filter{!it.terminal}.map{it.identity}.toSet()))}
        val selection=mutable.value.localPackageSelection
        if(selection!=null){
            try{
                val frame=LocalPackageView.parse(engine.localPackage(selection.key,selection.generation).await())
                require(frame.identity==selection)
                if(current===ready)mutable.update{if(it.localPackageSelection==selection)it.copy(localPackage=frame,localPackageUnavailable=false)else it}
            }catch(failure:Exception){
                if(failure is CancellationException)throw failure
                if(current===ready)mutable.update{if(it.localPackageSelection==selection)it.copy(localPackageUnavailable=true)else it}
            }
        }
    }
    fun importPackage(uri:android.net.Uri)=perform("package:localbegin") {engine->
        val handle=JSONObject(engine.importPackage(uri).await())
        val selection=LocalPackageSelection(handle.getString("key"),handle.getString("generation"))
        mutable.update{it.copy(localPackageSelection=selection,localPackage=null,localPackageUnavailable=false)}
        refreshPackages()
    }
    fun openLocalPackage(task:PackageTask){
        if(task.kind!="local_import")return
        mutable.update{it.copy(localPackageSelection=LocalPackageSelection(task.key,task.generation),localPackage=null,localPackageUnavailable=false)}
    }
    fun hideLocalPackage(){mutable.update{it.copy(localPackageSelection=null,localPackage=null,localPackageUnavailable=false)}}
    fun reviewLocalPackage()=localPackageAction("review"){engine,selection->engine.reviewLocalPackage(selection.key,selection.generation).await()}
    fun commitLocalPackage(token:String)=localPackageAction("commit"){engine,selection->engine.commitLocalPackage(selection.key,selection.generation,token).await()}
    private fun localPackageAction(action:String,block:suspend(ServiceSupervisor,LocalPackageSelection)->Unit){
        val selection=mutable.value.localPackageSelection?:return
        perform("package:local:$action:${selection.generation}"){engine->
            block(engine,selection);refreshPackages()
        }
    }
    fun setRegistryQuery(value:String){registrySearch.edit(value)}
    fun setRegistrySearchOrder(order: RegistrySearchOrder) = registrySearch.setOrder(order)
    fun setShowIncompatibleReleases(show: Boolean) = registrySearch.setShowIncompatible(show)
    fun cancelPackage(task:PackageTask){
        if(!task.cancellable||task.identity in mutable.value.packageCancels||!mutable.value.connected||mutable.value.packagesUnavailable)return
        mutable.update{it.copy(packageCancels=it.packageCancels+task.identity)}
        scope.launch{
            try{
                val engine=ready.await()
                val accepted=engine.cancelPackageOperation(task.key,task.generation)
                if(!accepted)mutable.update{it.copy(packageCancels=it.packageCancels-task.identity)}
                refreshPackages()
            }catch(failure:Exception){
                if(failure is CancellationException)throw failure
                mutable.update{it.copy(packagesUnavailable=true,packageCancels=it.packageCancels-task.identity)}
            }
        }
    }
    fun dismissPackage(task:PackageTask)=perform("package:dismiss:${task.identity}"){
        it.dismissPackage(task.key,task.generation).await();refreshPackages()
    }
    fun retryPackage(task:PackageTask)=perform("package:retry:${task.identity}",true){engine->
        try{engine.retryPackage(task.key,task.generation).await()}
        catch(failure:Exception){handlePackageFailure(failure)}
        refreshPackages()
    }
    private suspend fun handlePackageFailure(failure:Exception){
        if(failure is CancellationException)throw failure
        var actual:Throwable=failure
        while(actual is CompletionException&&actual.cause!=null)actual=actual.cause!!
        refreshPackages()
        val owned=actual as? PackageActionFailure
        if(owned==null||mutable.value.packages.none{it.key==owned.key&&it.generation==owned.generation&&it.terminal})throw failure
    }
    fun search(query:String)=registrySearch.search(query)
    fun nextRegistryPage()=registrySearch.next()
    fun retryRegistrySearch()=registrySearch.retry()
    fun cancelRegistrySearch()=registrySearch.cancel()
    fun pull(reference: String) = perform("pull:$reference",true) { engine->
        try{engine.pull(reference).await()}
        catch(failure:Exception){
            handlePackageFailure(failure)
        }
        refreshPackages()
    }
    fun create(spec: String) = perform("create",reportOperation=ReportOperation.Create) { engine -> engine.create("app-${UUID.randomUUID()}", spec).await() }
    private fun foreground() { getApplication<Application>().startForegroundService(Intent(getApplication(), EngineService::class.java)) }
    fun start(id: String) = perform(id,reportOperation=ReportOperation.Start) { foreground();it.start(id).await() }
    fun stop(id: String) = perform("stop:$id",reportOperation=ReportOperation.Stop) { it.stop(id).await() }
    fun restart(id: String) = perform(id,reportOperation=ReportOperation.Restart) { foreground();it.restart(id).await() }
    fun retry(id: String) = perform(id,reportOperation=ReportOperation.Restart) { foreground();it.retry(id).await() }
    fun remove(id: String) = perform(id,reportOperation=ReportOperation.Remove) { it.remove(id).await() }
    fun moreGroups()=perform("groups:page"){groupPages++}
    fun applicationAction(target:String,token:String,action:String)=perform("application:$action:$target",true,if(action=="start")ReportOperation.Start else if(action=="stop")ReportOperation.Stop else ReportOperation.Unknown){engine->
        if(action=="start")foreground()
        engine.shortcutAction(target,action,token,{true}).await()
    }
    fun removeApplication(target:String,token:String)=perform("application:remove:$target",true,ReportOperation.Remove){engine->
        engine.removeApplicationTarget(target,token).await()
    }
    fun openApplication(target:String,token:String)=perform("application:open:$target"){engine->
        refresh(engine)
        val value=JSONObject(engine.shortcutOpen(target,token,{true}).await())
        mutable.update{it.copy(shortcutWeb=WebTarget(value.getString("id"),value.getString("name"),"http://127.0.0.1:${value.getInt("host_port")}${value.getString("path")}",value.getString("generation"),value.getInt("instance_port"),value.getString("path")))}
    }
    fun consumeShortcutWeb(){mutable.update{it.copy(shortcutWeb=null)}}
    fun sharingTarget(target:String){mutable.update{it.copy(sharingTarget=target)}}
    fun shareApplication(id:String)=perform("sharing:open:$id") {engine->
        val target=JSONObject(engine.applicationTarget("application",id).await()).getJSONObject("target").toString()
        mutable.update{it.copy(sharingTarget=target)}
    }
    fun closeSharing(){mutable.update{it.copy(sharingTarget=null)}}
    internal fun sharingPermissionChanged() {
        connectedEngine?.sharingPermissionChanged()
    }
    suspend fun sharingStatus(target:String):String=ready.await().sharingStatus(target).await()
    internal suspend fun serviceSharingStatus(id: String, generation: String): String =
        ready.await().serviceSharingStatus(id, generation).await()
    internal suspend fun startSharing(target:String,stateToken:String,selectionToken:String,intent:dev.wasmdroid.app.engine.LanStartIntent):String =
        ready.await().startSharing(target,stateToken,selectionToken,::sharingAdmission,intent).await()
    internal fun sharingUiCurrent():Boolean=sharingAdmission()
    suspend fun stopSharing(target:String,epoch:Long):String=ready.await().stopSharing(target,epoch).await()
    fun clearApplicationFocus(){mutable.update{it.copy(applicationFocus=null)}}
    internal fun handleShortcutIntent(intent:Intent,current:()->Boolean){
        val api=dev.wasmdroid.app.shortcuts.ShortcutIntents
        val raw=intent.getStringExtra(api.EXTRA_CLICK)?:return
        scope.launch{
            try{
                val click=dev.wasmdroid.app.shortcuts.ShortcutClick.parse(raw)
                val target=click.selection.target?.json()?:error("Shortcut application is missing")
                val expected=if(intent.getBooleanExtra(api.EXTRA_REVIEW,false))dev.wasmdroid.app.shortcuts.ShortcutAction.Review else dev.wasmdroid.app.shortcuts.ShortcutAction.Open
                check(click.action==expected&&intent.getStringExtra(api.EXTRA_TARGET)==target&&intent.getStringExtra(api.EXTRA_STATE_TOKEN)==click.stateToken){"Shortcut action changed"}
                val guard={current()&&api.selectionCurrent(getApplication(),intent)}
                check(guard()){"Shortcut selection changed"}
                val engine=ready.await()
                if(expected==dev.wasmdroid.app.shortcuts.ShortcutAction.Review){
                    val record=JSONObject(engine.shortcutResolve(target).await())
                    check(guard()&&record.getString("state_token")==click.stateToken){"Application state changed; review it again"}
                    mutable.update{it.copy(applicationFocus=record.getJSONObject("target").toString())};refresh(engine)
                }else{
                    refresh(engine)
                    val value=JSONObject(engine.shortcutOpen(target,click.stateToken,guard).await())
                    if(guard())mutable.update{it.copy(shortcutWeb=WebTarget(value.getString("id"),value.getString("name"),"http://127.0.0.1:${value.getInt("host_port")}${value.getString("path")}",value.getString("generation"),value.getInt("instance_port"),value.getString("path")))}
                }
            }catch(failure:Exception){if(failure is CancellationException)throw failure;fail(failure)}
        }
    }
    suspend fun stackSnapshot(id:String):String=ready.await().stackSnapshot(id).await()
    fun startGroup(id:String)=perform("stack:start:$id",true,ReportOperation.Start){engine->
        foreground()
        try{engine.startStack(id).await()}
        catch(failure:Exception){
            if(failure is CancellationException)throw failure
            var actual:Throwable=failure
            while(actual.cause!=null)actual=actual.cause!!
            if(actual.message in setOf("Stack stopped during startup","Stack start cancelled","Service start cancelled"))
                mutable.update{it.copy(notice=dev.wasmdroid.app.R.string.group_start_cancelled)}
            else throw failure
        }
    }
    fun stopGroup(id:String)=perform("stack:stop:$id",true,ReportOperation.Stop){it.stopStack(id).await()}
    fun removeGroup(id:String,version:ULong,removed:()->Unit)=perform("stack:remove:$id",true,ReportOperation.Remove){it.removeStack(id,version).await();removed()}
    fun removeVolume(name: String) = perform("volume:$name",reportOperation=ReportOperation.Remove) { it.removeVolume(name).await() }
    fun removeImage(reference:String)=perform("image:$reference",reportOperation=ReportOperation.Remove){it.removeImage(reference).await()}
    fun collectUnusedCache()=perform("cache"){it.collectUnusedCache().await();mutable.update{state->state.copy(notice=dev.wasmdroid.app.R.string.cache_cleaned)}}
    fun advanced(value: Boolean) {
        preferences.edit { putBoolean("advanced", value) }
        mutable.update { it.copy(advanced = value) }
    }
    fun dismissError() { mutable.update { it.copy(error = null,errorKind=null,errorOperation=null,errorStage=null) } }
    fun dismissNotice(){mutable.update{it.copy(notice=null)}}
    suspend fun files(volume:String,path:String):List<String> = array(JSONArray(ready.await().dataEntries(volume,path).await()))
    fun createFolder(id:String,volume:String,path:String)=dataAction(id){engine,_->engine.createDirectory(volume,path).await()}
    fun deleteFile(id:String,volume:String,path:String,directory:Boolean)=dataAction(id){engine,_->engine.removeEntry(volume,path,directory).await()}
    fun importFile(id:String,volume:String,path:String,uri:android.net.Uri,overwrite:Boolean,expected:Long?)=
        dataAction(id){engine,operation->
            withContext(Dispatchers.IO){
                val resolver=getApplication<Application>().contentResolver
                val input=operation.external(resolver.openInputStream(uri)?:error("Source document unavailable"))
                val writer=operation.native(engine.beginImport(volume,path,overwrite,expected?.toULong(),spaceBudget(),operation.control).await())
                val buffer=ByteArray(64*1024);var total=0L;var last=0L
                while(true){ensureActive();val count=input.read(buffer);if(count<0)break;if(count==0)continue
                    writer.write(buffer.copyOf(count));total+=count
                    val now=android.os.SystemClock.elapsedRealtime();if(now-last>100){dataProgress("importing",total,expected);last=now}
                }
                ensureActive();writer.commit()
            }
        }
    fun exportFile(id:String,volume:String,path:String,uri:android.net.Uri)=dataAction(id){engine,operation->
        withContext(Dispatchers.IO){copyDocument(operation,operation.native(engine.beginExport(volume,path).await()),uri,"exporting")}
    }
    fun backup(id:String,volume:String,uri:android.net.Uri)=dataAction(id){engine,operation->
        withContext(Dispatchers.IO){copyDocument(operation,operation.native(engine.beginBackup(volume,operation.control).await()),uri,"backup")}
    }
    fun restore(id:String,volume:String,uri:android.net.Uri)=dataAction(id){engine,operation->
        val before=JSONObject(engine.inspect(id).await())
        withContext(Dispatchers.IO){
            val input=operation.external(getApplication<Application>().contentResolver.openInputStream(uri)?:error("Backup document unavailable"))
            val newVolume="restored-${UUID.randomUUID()}"
            val budget=spaceBudget()
            val restore=operation.native(engine.beginRestore(newVolume,budget,budget,operation.control).await())
            val buffer=ByteArray(64*1024);var total=0L;var last=0L
            while(true){ensureActive();val count=input.read(buffer);if(count<0)break;if(count==0)continue
                restore.write(buffer.copyOf(count));total+=count
                val now=android.os.SystemClock.elapsedRealtime();if(now-last>100){dataProgress("restoring",total,null);last=now}
            }
            ensureActive();dataProgress("restoring",total,null)
            val result=JSONObject(restore.commit(spaceBudget()))
            ensureActive()
            engine.attachRestored(id,before.getLong("version").toULong(),volume,result.getString("volume")).await()
        }
    }
    fun cancelData(id:String){dataOperations[id]?.let{it.cancel();it.job?.cancel()}}
    private fun dataAction(id:String,block:suspend(ServiceSupervisor,DataOperation)->Unit){
        if(id in dataOperations||id in mutable.value.busy)return
        val operation=DataOperation(DataTransferControl());dataOperations[id]=operation
        mutable.update{it.copy(busy=it.busy+"data:$id",error=null,errorKind=null,errorOperation=null,errorStage=null,notice=null)}
        operation.job=scope.launch{
            try{
                val engine=ready.await();operation.native(engine.beginDataOperation(id).await())
                block(engine,operation);refresh(engine)
                mutable.update{it.copy(notice=dev.wasmdroid.app.R.string.data_complete)}
            }catch(failure:Exception){if(failure !is CancellationException&&!operation.isCancelled)fail(failure,ReportOperation.Data)}
            finally{
                withContext(NonCancellable+Dispatchers.IO){
                    operation.finish()
                }
                dataOperations.remove(id)
                mutable.update{it.copy(busy=it.busy-"data:$id",progress=null,dataRevision=it.dataRevision+1)}
            }
        }
    }
    private fun spaceBudget():ULong{
        val free=getApplication<Application>().filesDir.availableTransferBytes()
        check(free>0){"Not enough free space for data transfer"}
        return free.toULong()
    }
    private suspend fun copyDocument(operation:DataOperation,reader:dev.wasmdroid.engine.DataReadTransfer,uri:android.net.Uri,phase:String){
        val output=operation.external(getApplication<Application>().contentResolver.openOutputStream(uri,"w")?:error("Destination document unavailable"))
        val expected=reader.sizeBytes()?.toLong();var total=0L;var last=0L
        while(true){kotlin.coroutines.coroutineContext.ensureActive();val bytes=reader.read(65536u);if(bytes.isEmpty())break
            output.write(bytes);total+=bytes.size
            val now=android.os.SystemClock.elapsedRealtime();if(now-last>100){dataProgress(phase,total,expected);last=now}
        }
        if(expected!=null)check(total==expected){"Source size changed during export"}
        output.flush();output.close()
    }
    private fun dataProgress(phase:String,bytes:Long,total:Long?){
        val value=JSONObject().put("phase",phase).put("bytes_downloaded",bytes).put("total_bytes",total?:0).toString()
        mutable.update{it.copy(progress=value)}
    }
    suspend fun output(id: String): String? = ready.await().output(id).await()
    internal suspend fun browserSession(target:WebTarget):dev.wasmdroid.app.engine.BrowserSessionLease =
        ready.await().acquireBrowserSession(target.id,target.generation,target.instancePort,target.path,false).await()
    internal suspend fun browserProfileState(id:String):String=ready.await().browserProfileState(id).await()
    internal suspend fun browserCapabilities():String=ready.await().browserCapabilities().await()
    internal suspend fun eraseBrowserProfile(id:String,revision:Long,profileToken:String):String=ready.await().eraseBrowserProfile(id,revision,profileToken).await()
    suspend fun logs(id:String,query:String):String?=ready.await().logs(id,query).await()
    suspend fun terminalSession(id:String):String=ready.await().terminalSession(id).await()
    suspend fun workerStats(id:String):String=ready.await().workerStats(id).await()
    suspend fun terminalFrame(id:String,generation:String,query:String):String=ready.await().terminalFrame(id,generation,query).await()
    suspend fun savedTerminal(id:String,generation:String,startRow:Int):String?=ready.await().savedTerminal(id,generation,startRow.toUShort()).await()
    suspend fun writeStdin(id:String,generation:String,bytes:ByteArray)=ready.await().writeStdin(id,generation,bytes).await()
    suspend fun closeStdin(id:String,generation:String)=ready.await().closeStdin(id,generation).await()
    suspend fun interrupt(id:String,generation:String)=ready.await().interrupt(id,generation).await()
    suspend fun resizeTerminal(id:String,generation:String,cols:Int,rows:Int,width:Int,height:Int)=ready.await().resizeTerminal(id,generation,cols,rows,width,height).await()
    private fun fail(failure: Throwable,operation:ReportOperation=ReportOperation.Unknown) {
        val actual = if (failure is CompletionException) failure.cause ?: failure else failure
        val message=(actual.message?:"Operation failed").replace(Regex("(?:https?|content|file)://\\S+"),"[resource]")
        mutable.update { it.copy(error = message,errorKind="other",errorOperation=operation.code,errorStage="failed") }
    }
    override fun onCleared() {
        storageUsage.close()
        registrySearch.close()
        configuration.close()
        stackImport.close()
        stackEdit.close()
        dataOperations.values.forEach{it.cancel()}
        scope.cancel()
        if (bound) getApplication<Application>().unbindService(connection)
        // Started foreground service ownership remains after UI closure.
    }
    companion object { fun array(array: JSONArray): List<String> = (0 until array.length()).map { array.getJSONObject(it).toString() } }
}
@OptIn(ExperimentalCoroutinesApi::class)
internal suspend fun <T> CompletableFuture<T>.await(): T = suspendCancellableCoroutine { continuation ->
    whenComplete { value, error ->
        if (error == null) continuation.resume(value){_,resource,_->(resource as? AutoCloseable)?.close()} else continuation.resumeWith(Result.failure(error))
    }
}

private class NativeGroupPermissionReview(val native:dev.wasmdroid.engine.GroupNetworkPermissionReview,override val source:GroupPermissionSource):GroupPermissionReview
