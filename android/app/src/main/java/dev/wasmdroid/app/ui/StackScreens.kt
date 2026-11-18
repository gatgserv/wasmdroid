package dev.wasmdroid.app.ui

import androidx.activity.compose.BackHandler
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.runtime.saveable.LocalSaveableStateRegistry
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.pluralStringResource
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.unit.dp
import dev.wasmdroid.app.R
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.delay
import kotlinx.coroutines.isActive
import org.json.JSONArray
import org.json.JSONObject

@OptIn(ExperimentalLayoutApi::class)
@Composable internal fun AppsDestination(
    state:AppState,model:AppModel,groups:Boolean,selectGroups:(Boolean)->Unit,
    groupId:String?,selectGroup:(String?)->Unit,importing:Boolean,importGroup:(Boolean)->Unit,
    open:(String)->Unit,logs:(String)->Unit,data:(String)->Unit,terminal:(String)->Unit,
    userStart:((()->Unit)->Unit),localNetworkGranted:Boolean,
){
    val draft by model.stackImport.state.collectAsState()
    val editing by model.stackEdit.state.collectAsState()
    LaunchedEffect(state.advanced){if(!state.advanced)model.stackEdit.leave(model.stackEdit.state.value.epoch)}
    LaunchedEffect(draft.createdId){draft.createdId?.let{id->
        selectGroups(true);selectGroup(id);importGroup(false);model.stackImport.clear()
    }}
    when{
        editing.open&&state.advanced->StackEditScreen(editing,state,model)
        importing&&state.advanced->StackImportScreen(state,draft,model.stackImport,model){importGroup(false)}
        groupId!=null->StackDetailsScreen(groupId,state,model,{selectGroup(null)},open,logs,data,terminal,userStart)
        else->Column(Modifier.fillMaxSize()){
            FlowRow(Modifier.padding(horizontal=20.dp),horizontalArrangement=Arrangement.spacedBy(8.dp)){
                FilterChip(selected=!groups,onClick={selectGroups(false)},label={Text(stringResource(R.string.apps_tab))})
                FilterChip(selected=groups,onClick={selectGroups(true)},label={Text(stringResource(R.string.groups))})
            }
            Box(Modifier.weight(1f)){
                if(groups)StackListScreen(state,model,{selectGroup(it)},{importGroup(true)})
                else AppsScreen(state,model,open,logs,data,terminal,userStart,localNetworkGranted)
            }
        }
    }
}

@Composable private fun StackListScreen(state:AppState,model:AppModel,details:(String)->Unit,importGroup:()->Unit){
    val byStack=remember(state.instances,state.memberships){state.instances.groupBy{state.memberships[JSONObject(it).getString("id")]}}
    LazyColumn(Modifier.fillMaxSize(),contentPadding=PaddingValues(20.dp),verticalArrangement=Arrangement.spacedBy(16.dp)){
        item{Text(stringResource(R.string.groups),style=MaterialTheme.typography.headlineMedium)}
        item{Text(stringResource(R.string.groups_description))}
        if(state.advanced)item{Button(enabled=state.connected,onClick=importGroup){Text(stringResource(R.string.group_import))}}
        if(!state.connected)item{Text(stringResource(R.string.connecting))}
        else if(state.groups.isEmpty())item{Text(stringResource(R.string.groups_empty))}
        items(state.groups,key={JSONObject(it).getString("id")}){json->
            val group=JSONObject(json);val id=group.getString("id");val instances=byStack[id].orEmpty()
            val running=instances.count{JSONObject(it).getString("state")=="running"}
            val serviceCount = group.getInt("service_count")
            val changing=groupBusy(state,id)||instances.any{state.controllers[JSONObject(it).getString("id")] in listOf("starting","stopping","data")}
            OutlinedCard(Modifier.fillMaxWidth()){
                Column(Modifier.padding(20.dp),verticalArrangement=Arrangement.spacedBy(10.dp)){
                    Text(id,style=MaterialTheme.typography.titleLarge)
                    Text(pluralStringResource(R.plurals.group_running_count, serviceCount, running, serviceCount))
                    if(changing)Text(stringResource(R.string.working),style=MaterialTheme.typography.labelLarge)
                    TextButton(onClick={details(id)}){Text(stringResource(R.string.group_details))}
                }
            }
        }
        if(state.moreGroups)item{OutlinedButton(enabled="groups:page" !in state.busy,onClick=model::moreGroups){Text(stringResource(R.string.group_more))}}
    }
}

private fun groupBusy(state:AppState,id:String)=listOf("start","stop","remove").any{"stack:$it:$id" in state.busy}

@OptIn(ExperimentalLayoutApi::class)
@Composable private fun StackDetailsScreen(
    id:String,state:AppState,model:AppModel,back:()->Unit,open:(String)->Unit,logs:(String)->Unit,
    data:(String)->Unit,terminal:(String)->Unit,userStart:((()->Unit)->Unit),
){
    var snapshot by remember(id){mutableStateOf<String?>(null)}
    var unavailable by remember(id){mutableStateOf(false)}
    var remove by remember(id){mutableStateOf(false)}
    BackHandler(onBack=back)
    LaunchedEffect(id){while(isActive){
        try{snapshot=model.stackSnapshot(id);unavailable=false}
        catch(failure:Exception){if(failure is CancellationException)throw failure;unavailable=true}
        delay(1500)
    }}
    val record=remember(snapshot){snapshot?.let(::JSONObject)}
    val members=record?.getJSONArray("members")?.let{AppModel.array(it)}.orEmpty()
    val instances=remember(state.instances){state.instances.associateBy{JSONObject(it).getString("id")}}
    val busy=groupBusy(state,id)
    val stopping="stack:stop:$id" in state.busy
    val removing="stack:remove:$id" in state.busy
    val live=members.any{member->val value=JSONObject(member);state.controllers[value.getString("instance_id")]!=null||value.getString("state") in listOf("running","paused","restarting")}
    val changing=members.any{state.controllers[JSONObject(it).getString("instance_id")] in listOf("starting","stopping","data")}
    val canUse=state.connected&&!unavailable&&record!=null
    LazyColumn(Modifier.fillMaxSize(),contentPadding=PaddingValues(20.dp),verticalArrangement=Arrangement.spacedBy(16.dp)){
        item{TextButton(onClick=back){Text(stringResource(R.string.back))}}
        item{Text(id,style=MaterialTheme.typography.headlineMedium)}
        item{Text(stringResource(R.string.group_stop_preserves))}
        if(unavailable)item{Text(stringResource(R.string.group_unavailable),color=MaterialTheme.colorScheme.error)}
        else if(record==null)item{LinearProgressIndicator(Modifier.fillMaxWidth())}
        item{FlowRow(horizontalArrangement=Arrangement.spacedBy(8.dp),verticalArrangement=Arrangement.spacedBy(8.dp)){
            Button(enabled=canUse&&!busy&&!changing&&state.runtimeAvailable&&members.isNotEmpty()&&members.none{JSONObject(it).getString("state") !in listOf("created","exited","running") }&&members.any{JSONObject(it).getString("state") in listOf("created","exited")},onClick={userStart{model.startGroup(id)}}){Text(stringResource(R.string.group_start))}
            OutlinedButton(enabled=canUse&&!stopping&&!removing&&(live||"stack:start:$id" in state.busy),onClick={model.stopGroup(id)}){Text(stringResource(R.string.group_stop))}
        }}
        if(state.advanced)item{OutlinedButton(enabled=canUse&&!busy,onClick={model.stackEdit.open(id)}){Text(stringResource(R.string.group_edit))}}
        if(busy)item{Text(stringResource(R.string.group_operation),style=MaterialTheme.typography.labelLarge)}
        items(members,key={JSONObject(it).getString("instance_id")}){memberJson->
            val member=JSONObject(memberJson);val instanceId=member.getString("instance_id")
            val json=instances[instanceId];val status=member.getString("state");val controller=state.controllers[instanceId]
            val instance=json?.let(::JSONObject)
            val failure=instance?.let(::serviceFailure)
            val active=controller in listOf("starting","running")||status in listOf("running","paused","restarting")
            val memberBusy=instanceId in state.busy||controller in listOf("stopping","data")
            val spec=instance?.getJSONObject("spec")
            OutlinedCard(Modifier.fillMaxWidth()){
                Column(Modifier.padding(20.dp),verticalArrangement=Arrangement.spacedBy(10.dp)){
                    Text(member.getString("service"),style=MaterialTheme.typography.titleLarge)
                    Text(stringResource(when(controller){"starting"->R.string.starting;"stopping"->R.string.working;"data"->R.string.working;else->if(failure!=null)R.string.failed else statusText(status)}))
                    if(instance!=null&&failure!=null&&controller!="starting")ServiceFailureMessage(instanceId,instance.optLong("version"),failure,state.connected,model)
                    if(status=="dead")Text(stringResource(R.string.group_retry_required),color=MaterialTheme.colorScheme.error)
                    if(status=="running"&&controller=="running")WorkerResourceSummary(instanceId,model)
                    FlowRow(horizontalArrangement=Arrangement.spacedBy(8.dp),verticalArrangement=Arrangement.spacedBy(4.dp)){
                        if(active)OutlinedButton(enabled=canUse&&!memberBusy&&!stopping&&!removing,onClick={model.stop(instanceId)}){Text(stringResource(R.string.stop))}
                        else Button(enabled=canUse&&!memberBusy&&!busy&&!changing&&state.runtimeAvailable,onClick={userStart{if(status=="dead")model.retry(instanceId)else model.start(instanceId)}}){Text(stringResource(if(status=="dead")R.string.retry else R.string.start))}
                        if(json!=null&&status=="running"&&webTarget(json,state)!=null)Button(enabled=canUse&&!memberBusy,onClick={open(json)}){Text(stringResource(R.string.open))}
                    }
                    FlowRow(horizontalArrangement=Arrangement.spacedBy(8.dp)){
                        TextButton(onClick={data(instanceId)}){Text(stringResource(R.string.manage_data))}
                        if(state.advanced||failure!=null){
                            TextButton(onClick={logs(instanceId)}){Text(stringResource(R.string.logs))}
                            if(state.advanced&&spec?.optBoolean("tty")==true&&status!="created")TextButton(onClick={terminal(instanceId)}){Text(stringResource(R.string.terminal))}
                        }
                    }
                    if(state.advanced&&spec!=null)Text(spec.getString("artifact_ref"),style=MaterialTheme.typography.bodySmall)
                }
            }
        }
        item{TextButton(enabled=canUse&&!busy&&!live&&members.isNotEmpty(),onClick={remove=true}){Text(stringResource(R.string.group_remove))}}
    }
    if(remove)AlertDialog(onDismissRequest={remove=false},title={Text(stringResource(R.string.group_remove_title,id))},text={Text(stringResource(R.string.group_remove_preserves))},
        confirmButton={TextButton(enabled=canUse&&!busy&&!live,onClick={record?.let{model.removeGroup(id,it.getLong("version").toULong(),back)};remove=false}){Text(stringResource(R.string.confirm))}},
        dismissButton={TextButton(onClick={remove=false}){Text(stringResource(R.string.cancel))}})
}

@Composable private fun StackImportScreen(state:AppState,draft:StackImportState,controller:StackImportController,model:AppModel,back:()->Unit){
    BackHandler(onBack=back)
    DisposableEffect(controller){onDispose{controller.leave()}}
    val picker=rememberLauncherForActivityResult(ActivityResultContracts.OpenDocument()){uri->if(uri!=null)controller.load(uri)}
    val plan=remember(draft.preview){draft.preview?.let(::JSONObject)}
    LazyColumn(Modifier.fillMaxSize(),contentPadding=PaddingValues(20.dp),verticalArrangement=Arrangement.spacedBy(16.dp)){
        item{TextButton(onClick=back){Text(stringResource(R.string.back))}}
        item{Text(stringResource(R.string.group_import),style=MaterialTheme.typography.headlineMedium)}
        item{Text(stringResource(R.string.group_import_description))}
        item{OutlinedTextField(value=draft.id,onValueChange={controller.edit(id=it)},enabled=!draft.busy,label={Text(stringResource(R.string.group_name))},singleLine=true,modifier=Modifier.fillMaxWidth())}
        item{OutlinedButton(enabled=!draft.busy,onClick={picker.launch(arrayOf("*/*"))}){Text(stringResource(R.string.group_choose_document))}}
        item{CompositionLocalProvider(LocalSaveableStateRegistry provides null){
            OutlinedTextField(value=draft.document,onValueChange={controller.edit(document=it)},enabled=!draft.busy,label={Text(stringResource(R.string.group_document))},minLines=6,maxLines=14,
                keyboardOptions=KeyboardOptions(autoCorrectEnabled=false),textStyle=MaterialTheme.typography.bodySmall.copy(fontFamily=FontFamily.Monospace),modifier=Modifier.fillMaxWidth())
        }}
        item{Button(enabled=state.connected&&!draft.busy&&draft.id.isNotBlank()&&draft.document.isNotBlank(),onClick=controller::preview){Text(stringResource(R.string.group_preview))}}
        draft.requirements?.let{requirements->item{StackPackageRequirements(requirements,state,model)}}
        if(draft.busy)item{LinearProgressIndicator(Modifier.fillMaxWidth())}
        draft.error?.let{error->item{Text(stringResource(error),color=MaterialTheme.colorScheme.error);draft.detail?.let{Text(it,style=MaterialTheme.typography.bodySmall)}}}
        if(plan!=null){
            item{StackPreview(plan)}
            item{Text(stringResource(R.string.group_create_no_start))}
            item{Button(enabled=state.connected&&!draft.busy,onClick=controller::create){Text(stringResource(R.string.group_create))}}
        }
        if(draft.document.isNotEmpty()||draft.id.isNotEmpty())item{TextButton(enabled=!draft.busy,onClick=controller::clear){Text(stringResource(R.string.group_clear_draft))}}
    }
}

@Composable internal fun StackPreview(plan:JSONObject){
    val services=plan.getJSONObject("services");val order=plan.getJSONArray("startup_order");val volumes=plan.getJSONObject("volumes")
    val serviceCount = services.length()
    val datasetCount = volumes.length()
    Column(verticalArrangement=Arrangement.spacedBy(12.dp)){
        Text(stringResource(R.string.group_preview),style=MaterialTheme.typography.titleLarge)
        Text(stringResource(
            R.string.group_preview_count,
            pluralStringResource(R.plurals.group_service_count, serviceCount, serviceCount),
            pluralStringResource(R.plurals.group_dataset_count, datasetCount, datasetCount),
        ))
        Text(stringResource(R.string.group_start_order,(0 until order.length()).joinToString(" → "){order.getString(it)}))
        Text(stringResource(R.string.group_private_network))
        for(index in 0 until order.length()){
            val name=order.getString(index);val spec=services.getJSONObject(name).getJSONObject("spec")
            OutlinedCard(Modifier.fillMaxWidth()){
                Column(Modifier.padding(16.dp),verticalArrangement=Arrangement.spacedBy(8.dp)){
                    Text(name,style=MaterialTheme.typography.titleMedium)
                    Text(spec.getString("artifact_ref"),style=MaterialTheme.typography.bodySmall)
                    Text(services.getJSONObject(name).getString("digest"),style=MaterialTheme.typography.bodySmall)
                    Text(stringResource(R.string.group_command,spec.getString("command")))
                    val args=spec.optJSONArray("args")?.length()?:0;val env=spec.optJSONObject("env")?.length()?:0
                    Text(
                        stringResource(
                            R.string.group_private_values,
                            pluralStringResource(R.plurals.group_argument_count, args, args),
                            pluralStringResource(R.plurals.group_environment_count, env, env),
                        ),
                        style = MaterialTheme.typography.bodySmall,
                    )
                    if(spec.optString("network")=="none")Text(stringResource(R.string.group_network_none))
                    val ports=spec.optJSONArray("ports")?:JSONArray()
                    for(portIndex in 0 until ports.length()){
                        val port=ports.getJSONObject(portIndex);val host=port.getInt("host_port")
                        Text(stringResource(R.string.group_port,port.getInt("instance_port"),if(host==0)stringResource(R.string.group_port_automatic)else host.toString()),style=MaterialTheme.typography.bodySmall)
                    }
                    val mounts=spec.optJSONArray("mounts")?:JSONArray()
                    for(mountIndex in 0 until mounts.length()){
                        val mount=mounts.getJSONObject(mountIndex)
                        Text(stringResource(R.string.group_mount,mount.getJSONObject("source").getString("source"),mount.getString("target"),
                            stringResource(if(mount.optBoolean("read_only"))R.string.group_read_only else R.string.group_read_write)),style=MaterialTheme.typography.bodySmall)
                    }
                }
            }
        }
        if(volumes.length()>0){
            Text(stringResource(R.string.group_datasets),style=MaterialTheme.typography.titleMedium)
            volumes.keys().forEach{name->val volume=volumes.getJSONObject(name)
                Text(stringResource(if(volume.getBoolean("external"))R.string.group_external_dataset else R.string.group_scoped_dataset,name,volume.getString("name")),style=MaterialTheme.typography.bodySmall)
            }
        }
    }
}
