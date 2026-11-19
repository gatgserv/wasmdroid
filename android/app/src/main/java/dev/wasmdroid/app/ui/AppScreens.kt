package dev.wasmdroid.app.ui

import android.Manifest
import android.content.pm.PackageManager
import android.os.Build
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.isSystemInDarkTheme
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.unit.dp
import dev.wasmdroid.app.R
import dev.wasmdroid.app.engine.PackageRuntimeRequirements
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import org.json.JSONArray
import org.json.JSONObject

private val Light = lightColorScheme(primary=Color(0xFF245EC7),secondary=Color(0xFF15776E),background=Color(0xFFF7F8FC),surface=Color(0xFFF7F8FC))
private val Dark = darkColorScheme(primary=Color(0xFFAEC6FF),secondary=Color(0xFF83D5C8),background=Color(0xFF10131B),surface=Color(0xFF10131B))
@OptIn(ExperimentalMaterial3Api::class)
@Composable fun WasmDroidApp(model:AppModel) {
    val state by model.state.collectAsState()
    val downloadedPackages = remember(state.images) {
        state.images.associateBy { JSONObject(it).getString("reference") }
    }
    var tab by rememberSaveable {mutableIntStateOf(0)}
    var logs by remember {mutableStateOf<String?>(null)}
    var web by remember {mutableStateOf<WebTarget?>(null)}
    var dataId by rememberSaveable{mutableStateOf<String?>(null)}
    var terminalId by rememberSaveable{mutableStateOf<String?>(null)}
    var groups by rememberSaveable{mutableStateOf(false)}
    var groupId by rememberSaveable{mutableStateOf<String?>(null)}
    var groupImport by rememberSaveable{mutableStateOf(false)}
    val context=LocalContext.current
    val localNetwork = rememberLocalNetworkAccess()
    LaunchedEffect(localNetwork.granted, state.connected) {
        if (state.connected) model.sharingPermissionChanged()
    }
    var pendingStart by remember {mutableStateOf<(() -> Unit)?>(null)}
    val notifications=rememberLauncherForActivityResult(ActivityResultContracts.RequestPermission()) {
        pendingStart?.invoke();pendingStart=null
    }
    val userStart:((()->Unit)->Unit)={action->
        if(Build.VERSION.SDK_INT>=33&&context.checkSelfPermission(Manifest.permission.POST_NOTIFICATIONS)!=PackageManager.PERMISSION_GRANTED){
            pendingStart=action;notifications.launch(Manifest.permission.POST_NOTIFICATIONS)
        }else action()
    }
    LaunchedEffect(state.shortcutWeb){state.shortcutWeb?.let{web=it;model.consumeShortcutWeb()}}
    LaunchedEffect(state.applicationFocus){if(state.applicationFocus!=null){tab=1;groups=false;groupId=null;groupImport=false}}
    MaterialTheme(colorScheme=if(isSystemInDarkTheme()) Dark else Light) {
        if(web!=null) {
            val target=web!!
            val alive=state.instances.any{webTarget(it,state)?.sameEndpoint(target)==true&&JSONObject(it).getString("state")=="running"}
            LaunchedEffect(alive){if(!alive)web=null}
            LocalWebScreen(target,model){web=null}
        } else if(terminalId!=null&&state.advanced){
            val id=terminalId!!
            val record=state.instances.firstOrNull{JSONObject(it).getString("id")==id}?.let(::JSONObject)
            TerminalScreen(id,record?.getJSONObject("spec")?.optJSONObject("labels")?.optString("dev.wasmdroid.ui.name")?.ifBlank{null}?:id,model){terminalId=null}
        } else if(logs!=null){
            val id=logs!!
            val record=state.instances.firstOrNull{JSONObject(it).getString("id")==id}?.let(::JSONObject)
            LiveLogsScreen(id,record?.getJSONObject("spec")?.optJSONObject("labels")?.optString("dev.wasmdroid.ui.name")?.ifBlank{null}?:id,model){logs=null}
        } else if(dataId!=null){
            val record=state.instances.firstOrNull{JSONObject(it).getString("id")==dataId}
            if(record!=null)AppDataScreen(record,state,model){dataId=null}
            else LaunchedEffect(dataId){dataId=null}
        } else {
            Scaffold(topBar={TopAppBar(title={Text("WasmDroid")},actions={
                Text(stringResource(if(state.connected) R.string.this_phone else R.string.connecting),style=MaterialTheme.typography.labelMedium,modifier=Modifier.padding(end=20.dp))
            })},bottomBar={NavigationBar {
                listOf(R.string.store,R.string.my_apps,R.string.settings).forEachIndexed{index,label->
                    NavigationBarItem(selected=tab==index,onClick={tab=index;model.clearApplicationFocus()},icon={NavGlyph(index)},label={Text(stringResource(label))})
                }
            }}) {padding->Column(Modifier.fillMaxSize().padding(padding)) {
                if(state.connected&&!state.runtimeAvailable)Text(stringResource(R.string.runtime_missing),modifier=Modifier.padding(20.dp))
                state.error?.let{ErrorCard(it,state.advanced,dev.wasmdroid.app.reports.ReportIssue.fromCodes(state.errorKind,state.errorOperation,state.errorStage),model::dismissError)}
                state.notice?.let{NoticeCard(it,model::dismissNotice)}
                PackageOperationsPane(state,model,downloadedPackages)
                if(state.progress!=null)OperationProgress(state.progress)
                else if(state.busy.any{!packageBusyKey(it)})OperationProgress(null)
                when(tab){
                    0->StoreScreen(state,model,downloadedPackages)
                    1->AppsDestination(state,model,groups,{groups=it},groupId,{groupId=it},groupImport,{groupImport=it},
                        {web=webTarget(it,state)},{logs=it},{dataId=it},{terminalId=it},userStart,localNetwork.granted)
                    2->SettingsScreen(state,model)
                }
            }}
            if(state.localPackageSelection!=null)LocalPackageImportDialog(state,model,
                {spec->model.hideLocalPackage();model.configuration.create(spec)},
                {query->model.hideLocalPackage();tab=0;model.setRegistryQuery(query);model.search(query)})
        }
        ServiceConfigurationScreen(model.configuration,state)
        AutomaticStartDialog(model.automaticStart, state)
        NetworkPermissionDialog(model.networkPermissions,state,localNetwork)
        GroupNetworkPermissionDialog(model.groupNetworkPermissions,state,localNetwork)
        state.sharingTarget?.let{target->LanSharingDialog(target,model,localNetwork,model::closeSharing)}
    }
}
@Composable private fun NavGlyph(index:Int){
    val color=LocalContentColor.current
    Canvas(Modifier.size(24.dp)){
        val u=size.width/24;val stroke=Stroke(1.8f*u)
        when(index){
            0->{drawRect(color,Offset(4*u,9*u),Size(16*u,11*u),style=stroke);drawLine(color,Offset(3*u,9*u),Offset(21*u,9*u),1.8f*u);drawLine(color,Offset(5*u,4*u),Offset(19*u,4*u),1.8f*u);drawLine(color,Offset(3*u,9*u),Offset(5*u,4*u),1.8f*u);drawLine(color,Offset(21*u,9*u),Offset(19*u,4*u),1.8f*u)}
            1->for(x in listOf(4,14))for(y in listOf(4,14))drawRect(color,Offset(x*u,y*u),Size(6*u,6*u),style=stroke)
            else->{for(y in listOf(6,12,18))drawLine(color,Offset(3*u,y*u),Offset(21*u,y*u),1.8f*u);for((x,y)in listOf(8 to 6,16 to 12,10 to 18))drawCircle(color,2.6f*u,Offset(x*u,y*u))}
        }
    }
}
@Composable private fun Intro(title:Int,subtitle:Int){Column(Modifier.padding(vertical=8.dp),verticalArrangement=Arrangement.spacedBy(8.dp)){Text(stringResource(title),style=MaterialTheme.typography.headlineMedium);Text(stringResource(subtitle),color=MaterialTheme.colorScheme.onSurfaceVariant)}}
@Composable private fun EmptyCard(title:Int,body:Int){OutlinedCard(Modifier.fillMaxWidth()){Column(Modifier.padding(24.dp),verticalArrangement=Arrangement.spacedBy(12.dp)){Text(stringResource(title),style=MaterialTheme.typography.titleLarge);Text(stringResource(body),color=MaterialTheme.colorScheme.onSurfaceVariant)}}}

@Composable
private fun StoreScreen(state: AppState, model: AppModel, downloadedPackages: Map<String, String>) {
    val importPicker = rememberLauncherForActivityResult(ActivityResultContracts.OpenDocument()) { uri ->
        if (uri != null) model.importPackage(uri)
    }
    var removeImage by remember { mutableStateOf<String?>(null) }
    val listState = rememberLazyListState()
    val scope = rememberCoroutineScope()

    Column(Modifier.fillMaxSize()) {
        RegistrySearchForm(state, model) { scope.launch { listState.scrollToItem(0) } }
        LazyColumn(
            Modifier.weight(1f),
            state = listState,
            contentPadding = PaddingValues(20.dp),
            verticalArrangement = Arrangement.spacedBy(16.dp),
        ) {
            if (state.registrySearch.requestedQuery == null) item {
                Intro(R.string.store_title, R.string.store_subtitle)
                Text(stringResource(R.string.search_public_connection), style = MaterialTheme.typography.bodySmall)
            }
            registrySearchItems(state, model, downloadedPackages)
            if (state.advanced) {
                item {
                    OutlinedButton(
                        enabled = state.connected && state.runtimeAvailable && !state.packageWorkActive() && !state.packagesUnavailable,
                        onClick = { importPicker.launch(arrayOf("*/*")) },
                    ) { Text(stringResource(R.string.local_package_import)) }
                }
            }
            if (state.images.isNotEmpty()) {
                item { Text(stringResource(R.string.downloaded_images), style = MaterialTheme.typography.titleLarge) }
            }
            items(state.images, key = { JSONObject(it).getString("reference") }) { json ->
                val image = remember(json) { JSONObject(json) }
                val reference = image.getString("reference")
                val commands = image.getJSONArray("commands")
                val removing = "image:$reference" in state.busy
                OutlinedCard(Modifier.fillMaxWidth()) {
                    Column(Modifier.padding(16.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
                        Text(reference, style = MaterialTheme.typography.titleMedium)
                        AndroidTestedLabel(reference)
                        ArtifactSize(image)
                        if (state.advanced) {
                            Text((0 until commands.length()).joinToString { commands.getString(it) })
                        }
                        CreateDownloadedPackageServiceButton(json, state, model)
                        TextButton(enabled = state.connected && !removing, onClick = { removeImage = reference }) {
                            Text(stringResource(R.string.remove_package))
                        }
                    }
                }
            }
        }
    }
    removeImage?.let { reference ->
        AlertDialog(
            onDismissRequest = { removeImage = null },
            title = { Text(stringResource(R.string.remove_package)) },
            text = { Text(reference + "\n\n" + stringResource(R.string.remove_package_body)) },
            confirmButton = {
                TextButton(onClick = { model.removeImage(reference); removeImage = null }) {
                    Text(stringResource(R.string.confirm))
                }
            },
            dismissButton = { TextButton(onClick = { removeImage = null }) { Text(stringResource(R.string.cancel)) } },
        )
    }
}

internal fun downloadedPackageServiceSpec(state: AppState, imageJson: String): String? {
    if (!state.connected || !state.runtimeAvailable || imageJson !in state.images) return null
    val image = JSONObject(imageJson)
    val reference = image.getString("reference")
    val commands = image.getJSONArray("commands").let { array ->
        (0 until array.length()).map(array::getString)
    }
    val command = image.optJSONObject("labels")?.optString("dev.wasmdroid.entrypoint")
        ?.takeIf { it in commands } ?: commands.firstOrNull()
    if (command.isNullOrBlank() || "image:$reference" in state.busy) return null
    return JSONObject()
        .put("artifact_ref", reference)
        .put("command", command)
        .put("args", JSONArray())
        .put("env", JSONObject())
        .put("mounts", JSONArray())
        .put("ports", JSONArray())
        .put("network", "loopback")
        .toString(2)
}

@Composable
internal fun CreateDownloadedPackageServiceButton(imageJson: String, state: AppState, model: AppModel) {
    OutlinedButton(
        enabled = downloadedPackageServiceSpec(state, imageJson) != null,
        onClick = {
            downloadedPackageServiceSpec(model.state.value, imageJson)?.let(model.configuration::create)
        },
    ) { Text(stringResource(R.string.create_service)) }
}
@Composable internal fun AppsScreen(state:AppState,model:AppModel,open:(String)->Unit,logs:(String)->Unit,data:(String)->Unit,terminal:(String)->Unit,userStart:((()->Unit)->Unit),localNetworkGranted:Boolean){
    var remove by remember{mutableStateOf<String?>(null)}
    val instances=remember(state.instances,state.memberships){state.instances.filter{JSONObject(it).getString("id") !in state.memberships}}
    val installed = remember(state.images) {
        state.images.map(::JSONObject).associateBy { it.optString("reference") }
    }
    LazyColumn(Modifier.fillMaxSize(),contentPadding=PaddingValues(20.dp),verticalArrangement=Arrangement.spacedBy(16.dp)){
        item{Intro(R.string.my_apps,R.string.apps_subtitle)}
        combinedApplications(state,model,data,logs,terminal,userStart,localNetworkGranted)
        if(instances.isEmpty()&&state.applications.isEmpty())item{EmptyCard(R.string.apps_empty,R.string.apps_empty_body)}
        items(instances,key={JSONObject(it).getString("id")}){json->
            val row=JSONObject(json);val id=row.getString("id");val spec=row.getJSONObject("spec");val status=row.getString("state");val running=status=="running";val pending=state.controllers[id] in setOf("starting","restarting");val busy=id in state.busy||state.controllers[id] in listOf("starting","stopping","data")
            val failure=serviceFailure(row)
            val endpoint = webTarget(json, state)
            val needsSetup = PackageRuntimeRequirements.missing(spec, installed[spec.optString("artifact_ref")]).isNotEmpty()
            val name=spec.optJSONObject("labels")?.optString("dev.wasmdroid.ui.name")?.takeIf{it.isNotBlank()}?:spec.getString("command")
            OutlinedCard(Modifier.fillMaxWidth()){Column(Modifier.padding(20.dp),verticalArrangement=Arrangement.spacedBy(12.dp)){
                Row(Modifier.fillMaxWidth(),horizontalArrangement=Arrangement.SpaceBetween){Text(name,style=MaterialTheme.typography.titleLarge,modifier=Modifier.weight(1f));Text(stringResource(if(state.controllers[id]=="restarting")R.string.application_restart_pending else if(busy)R.string.working else if(failure!=null)R.string.failed else statusText(status)),style=MaterialTheme.typography.labelLarge,color=if(running)MaterialTheme.colorScheme.secondary else MaterialTheme.colorScheme.onSurfaceVariant)}
                if(failure!=null&&!pending)ServiceFailureMessage(id,row.getLong("version"),failure,state.connected,model)
                if(running)WorkerResourceSummary(id,model)
                if (running && (spec.optJSONArray("ports")?.length() ?: 0) == 0) {
                    Text(stringResource(R.string.lan_no_published_ports), style = MaterialTheme.typography.bodySmall)
                }
                if (running && endpoint != null && state.connected && localNetworkGranted && "stop:$id" !in state.busy) {
                    LanCardAddresses("$id:${endpoint.generation}:${endpoint.instancePort}:${endpoint.path}") {
                        model.serviceSharingStatus(id, endpoint.generation)
                    }
                }
                Row(horizontalArrangement=Arrangement.spacedBy(8.dp)){
                    if(running||pending){
                        if(endpoint!=null)Button(enabled=!busy,onClick={open(json)}){Text(stringResource(R.string.open))}
                        OutlinedButton(enabled="stop:$id" !in state.busy&&state.controllers[id] !in setOf("stopping","data"),onClick={model.stop(id)}){Text(stringResource(R.string.stop))}
                    } else Button(enabled = !busy && state.runtimeAvailable && state.connected, onClick = {
                        if (needsSetup) model.configuration.edit(id, initialTab = 1)
                        else userStart { if (status == "dead") model.retry(id) else model.start(id) }
                    }) {
                        Text(stringResource(when {
                            needsSetup -> R.string.config_package_setup
                            status == "dead" || failure != null -> R.string.retry
                            else -> R.string.start
                        }))
                    }
                }
                if(state.advanced) Text(spec.getString("artifact_ref"),style=MaterialTheme.typography.bodySmall)
                if(status in listOf("created","exited","dead"))
                    TextButton(enabled=!busy&&state.connected&&state.runtimeAvailable,
                        onClick={model.configuration.edit(id)}){Text(stringResource(R.string.config_edit))}
                if(state.advanced){
                    Row(horizontalArrangement=Arrangement.spacedBy(8.dp)){
                        TextButton(onClick={logs(id)}){Text(stringResource(R.string.logs))}
                        if(spec.optBoolean("tty")&&status!="created")TextButton(onClick={terminal(id)}){Text(stringResource(R.string.terminal))}
                        if(running)TextButton(enabled=!busy,onClick={userStart{model.restart(id)}}){Text(stringResource(R.string.restart))}
                    }
                }
                if (!state.advanced && status != "created")
                    TextButton(onClick={logs(id)}){Text(stringResource(R.string.logs))}
                if(!running)TextButton(enabled=!busy,onClick={remove=id}){Text(stringResource(R.string.remove))}
                TextButton(enabled=state.connected,onClick={model.networkPermissions.edit(id)}){Text(stringResource(R.string.network_permission_title))}
                if (!state.advanced && state.runtimeAvailable) {
                    TextButton(enabled = state.connected, onClick = { model.automaticStart.edit(id) }) {
                        Text(stringResource(R.string.automation_title))
                    }
                }
                if(endpoint!=null)TextButton(enabled=!busy&&state.connected,onClick={model.shareApplication(id)}){Text(stringResource(R.string.lan_share))}
                TextButton(onClick={data(id)}){Text(stringResource(R.string.manage_data))}
            }}
        }
    }
    remove?.let{id->ConfirmDialog(R.string.remove_app,R.string.remove_keeps_data,{remove=null}){model.remove(id);remove=null}}
}
internal fun statusText(state:String)=when(state){"running"->R.string.running;"created","exited"->R.string.stopped;"restarting"->R.string.starting;"paused"->R.string.paused;else->R.string.failed}
@Composable private fun SettingsScreen(state:AppState,model:AppModel){
    var delete by remember{mutableStateOf<String?>(null)}
    var document by remember{mutableStateOf<Pair<Int, String>?>(null)}
    val context=LocalContext.current
    LazyColumn(Modifier.fillMaxSize(),contentPadding=PaddingValues(20.dp),verticalArrangement=Arrangement.spacedBy(20.dp)){
        item{Intro(R.string.settings,R.string.settings_subtitle)}
        item{OutlinedCard{Row(Modifier.fillMaxWidth().padding(20.dp),horizontalArrangement=Arrangement.spacedBy(16.dp)){
            Column(Modifier.weight(1f),verticalArrangement=Arrangement.spacedBy(8.dp)){Text(stringResource(R.string.advanced_mode),style=MaterialTheme.typography.titleMedium);Text(stringResource(R.string.advanced_body))}
            Switch(checked=state.advanced,onCheckedChange=model::advanced)
        }}}
        item{Text(stringResource(R.string.saved_volumes,state.volumes.size))}
        item{StorageUsagePane(state.storageUsage,state.connected,model.storageUsage::refresh)}
        item{OutlinedButton(enabled="cache" !in state.busy,onClick=model::collectUnusedCache){Text(stringResource(R.string.clear_unused_cache))};Text(stringResource(R.string.cache_preserves_data),style=MaterialTheme.typography.bodySmall)}
        item {
            Text("WasmDroid · Apache-2.0", style = MaterialTheme.typography.titleMedium)
            Text(stringResource(R.string.no_accounts))
            TextButton(onClick = { document = R.string.licenses to "THIRD_PARTY_NOTICES.md" }) {
                Text(stringResource(R.string.licenses))
            }
            TextButton(onClick = { document = R.string.privacy_policy to "privacy-policy.txt" }) {
                Text(stringResource(R.string.privacy_policy))
            }
        }
        if(state.advanced)items(state.volumes,key={JSONObject(it).getString("name")}){json->val name=JSONObject(json).getString("name");OutlinedCard{Column(Modifier.fillMaxWidth().padding(16.dp)){Text(name);TextButton(onClick={delete=name}){Text(stringResource(R.string.delete_data))}}}}
    }
    delete?.let{name->AlertDialog(onDismissRequest={delete=null},title={Text(stringResource(R.string.delete_data))},text={Text(name+"\n\n"+stringResource(R.string.delete_data_warning))},
        confirmButton={TextButton(onClick={model.removeVolume(name);delete=null}){Text(stringResource(R.string.confirm))}},dismissButton={TextButton(onClick={delete=null}){Text(stringResource(R.string.cancel))}})}
    document?.let { (title, asset) ->
        var text by remember(asset) { mutableStateOf("") }
        LaunchedEffect(asset) {
            text = withContext(Dispatchers.IO) {
                context.assets.open(asset).bufferedReader().use { it.readText() }
            }
        }
        AlertDialog(
            onDismissRequest = { document = null },
            title = { Text(stringResource(title)) },
            text = { Text(text, modifier = Modifier.heightIn(max = 450.dp).verticalScroll(rememberScrollState())) },
            confirmButton = { TextButton(onClick = { document = null }) { Text(stringResource(R.string.close)) } },
        )
    }
}
@Composable internal fun ErrorCard(error:String,advanced:Boolean,dismiss:()->Unit){
    ErrorCard(error,advanced,dev.wasmdroid.app.reports.ReportIssue.fromCodes(null,null,"failed"),dismiss)
}
@Composable internal fun ErrorCard(error:String,advanced:Boolean,issue:dev.wasmdroid.app.reports.ReportIssue,dismiss:()->Unit){
    val message=when{error.contains("Package operation",true)||error.contains("package cache writer",true)->R.string.package_busy;error.contains("Not enough free workers",true)||error.contains("No free execution worker",true)->R.string.group_capacity;error.contains("active leases",true)||error.contains("Stop service",true)->R.string.data_stop_first;error.contains("space",true)||error.contains("os error 28")->R.string.error_space;error.contains("digest",true)->R.string.error_integrity;error.contains("unsupported",true)->R.string.error_unsupported;error.contains("network",true)||error.contains("connection",true)->R.string.error_network;else->R.string.error_generic}
    Card(Modifier.padding(horizontal=20.dp,vertical=8.dp),colors=CardDefaults.cardColors(containerColor=MaterialTheme.colorScheme.errorContainer)){Column(Modifier.padding(16.dp)){
        Text(stringResource(message));if(advanced)Text(error,style=MaterialTheme.typography.bodySmall)
        DiagnosticReportButton(issue)
        TextButton(onClick=dismiss){Text(stringResource(R.string.dismiss))}
    }}
}
@Composable internal fun OperationProgress(raw:String?){
    val event=remember(raw){runCatching{JSONObject(raw?:"")}.getOrNull()}
    val text=when(event?.optString("phase")){"importing"->R.string.importing;"exporting","backup"->R.string.exporting;"restoring"->R.string.restoring;"downloading"->R.string.downloading;"verifying"->R.string.verifying;"installed"->R.string.installed;else->R.string.preparing}
    val transferred=event?.optLong("bytes_downloaded",0)?:0
    val total=event?.optLong("total_bytes",0)?:0
    Column(Modifier.padding(horizontal=20.dp,vertical=8.dp),verticalArrangement=Arrangement.spacedBy(8.dp)){
        Text(stringResource(text),style=MaterialTheme.typography.labelLarge)
        if(transferred>0)Text(stringResource(R.string.transferred_kib,transferred/1024),style=MaterialTheme.typography.bodySmall)
        if(total>0)LinearProgressIndicator(progress={(transferred.toFloat()/total).coerceIn(0f,1f)},modifier=Modifier.fillMaxWidth())
        else LinearProgressIndicator(Modifier.fillMaxWidth())
    }
}
@Composable internal fun NoticeCard(text:Int,dismiss:()->Unit){
    Card(Modifier.padding(horizontal=20.dp,vertical=8.dp)){Row(Modifier.fillMaxWidth().padding(16.dp),horizontalArrangement=Arrangement.spacedBy(8.dp)){
        Text(stringResource(text),modifier=Modifier.weight(1f));TextButton(onClick=dismiss){Text(stringResource(R.string.dismiss))}
    }}
}
@Composable private fun ConfirmDialog(title:Int,body:Int,dismiss:()->Unit,confirm:()->Unit){AlertDialog(onDismissRequest=dismiss,title={Text(stringResource(title))},text={Text(stringResource(body))},confirmButton={TextButton(onClick=confirm){Text(stringResource(R.string.confirm))}},dismissButton={TextButton(onClick=dismiss){Text(stringResource(R.string.cancel))}})}
