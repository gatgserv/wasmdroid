package dev.wasmdroid.app.ui

import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.LazyListScope
import androidx.compose.foundation.lazy.items
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.unit.dp
import dev.wasmdroid.app.R
import org.json.JSONObject

internal fun LazyListScope.combinedApplications(
    state:AppState,model:AppModel,data:(String)->Unit,logs:(String)->Unit,terminal:(String)->Unit,
    userStart:((()->Unit)->Unit),localNetworkGranted:Boolean,
){
    items(state.applications,key={raw->JSONObject(raw).getJSONObject("target").getString("identity_token")}){raw->
        CombinedApplicationCard(raw,state,model,data,logs,terminal,userStart,localNetworkGranted)
    }
    if(state.moreGroups)item{OutlinedButton(enabled=state.connected&&"groups:page" !in state.busy,onClick=model::moreGroups){Text(stringResource(R.string.application_more))}}
}

@OptIn(ExperimentalLayoutApi::class)
@Composable private fun CombinedApplicationCard(
    raw:String,state:AppState,model:AppModel,data:(String)->Unit,logs:(String)->Unit,terminal:(String)->Unit,
    userStart:((()->Unit)->Unit),localNetworkGranted:Boolean,
){
    val record=remember(raw){JSONObject(raw)};val target=record.getJSONObject("target").toString()
    val token=record.getString("state_token");val status=record.getString("state");val name=record.getString("name")
    var details by remember{mutableStateOf(false)}
    var removal by remember{mutableStateOf<Pair<String,String>?>(null)}
    LaunchedEffect(state.applicationFocus){if(state.applicationFocus==target)details=true}
    val busy=state.busy.any{it.endsWith(":$target")}
    val members=record.getJSONArray("members")
    OutlinedCard(Modifier.fillMaxWidth()){
        Column(Modifier.padding(20.dp),verticalArrangement=Arrangement.spacedBy(12.dp)){
            Text(name,style=MaterialTheme.typography.titleLarge)
            Text(stringResource(when(status){"starting"->R.string.starting;"running"->R.string.running;"partial"->R.string.application_partial;"stopping"->R.string.application_stopping;"stopped"->R.string.stopped;else->R.string.application_unavailable}),style=MaterialTheme.typography.labelLarge)
            if(record.optBoolean("has_failure"))Text(stringResource(R.string.application_failed),color=MaterialTheme.colorScheme.error)
            if (record.getBoolean("can_open") && state.connected && localNetworkGranted && "application:stop:$target" !in state.busy) {
                LanCardAddresses(token) { model.sharingStatus(target) }
            }
            FlowRow(horizontalArrangement=Arrangement.spacedBy(8.dp),verticalArrangement=Arrangement.spacedBy(6.dp)){
                if(record.getBoolean("can_start"))Button(enabled=state.connected&&state.runtimeAvailable&&!busy,onClick={userStart{model.applicationAction(target,token,"start")}}){Text(stringResource(if(status=="partial")R.string.application_start_missing else R.string.start))}
                if(record.getBoolean("can_stop"))OutlinedButton(enabled=state.connected&&"application:stop:$target" !in state.busy,onClick={model.applicationAction(target,token,"stop")}){Text(stringResource(R.string.stop))}
                if(record.getBoolean("can_open"))Button(enabled=state.connected&&!busy,onClick={model.openApplication(target,token)}){Text(stringResource(R.string.open))}
            }
            if(busy||status in setOf("starting","stopping"))LinearProgressIndicator(Modifier.fillMaxWidth())
            FlowRow(horizontalArrangement=Arrangement.spacedBy(8.dp)){
                TextButton(enabled=state.connected&&!busy,onClick={model.groupNetworkPermissions.edit(target)}){Text(stringResource(R.string.network_permission_title))}
                TextButton(onClick={details=!details}){Text(stringResource(if(details)R.string.application_hide_details else R.string.application_details))}
                TextButton(onClick={details=true}){Text(stringResource(R.string.manage_data))}
                if(record.getBoolean("can_open"))TextButton(enabled=state.connected&&!busy,onClick={model.sharingTarget(target)}){Text(stringResource(R.string.lan_share))}
                if(state.advanced)TextButton(enabled=state.connected&&!busy,onClick={model.stackEdit.open(record.getJSONObject("target").getString("id"))}){Text(stringResource(R.string.group_edit))}
            }
            if(details){
                Text(stringResource(R.string.application_parts),style=MaterialTheme.typography.titleMedium)
                for(index in 0 until members.length()){
                    val member=members.getJSONObject(index);val id=member.getString("id")
                    val label=if(state.advanced)member.getString("name")else stringResource(R.string.application_part,index+1)
                    Column(verticalArrangement=Arrangement.spacedBy(4.dp)){
                        Text(stringResource(R.string.application_part_state,label,stringResource(statusText(member.getString("state")))))
                        FlowRow(horizontalArrangement=Arrangement.spacedBy(8.dp)){
                            TextButton(enabled=state.connected&&!busy,onClick={data(id)}){Text(stringResource(R.string.application_part_data,label))}
                            if(state.advanced){
                                TextButton(onClick={logs(id)}){Text(stringResource(R.string.logs))}
                                val row=state.instances.firstOrNull{JSONObject(it).getString("id")==id}?.let(::JSONObject)
                                if(row?.getJSONObject("spec")?.optBoolean("tty")==true&&member.getString("state")!="created")TextButton(onClick={terminal(id)}){Text(stringResource(R.string.terminal))}
                            }
                        }
                    }
                }
                Text(stringResource(R.string.application_preserve),style=MaterialTheme.typography.bodySmall)
            }
            if(status=="stopped")TextButton(enabled=state.connected&&!busy,onClick={removal=target to token}){Text(stringResource(R.string.remove_app))}
        }
    }
    removal?.let{captured->AlertDialog(onDismissRequest={removal=null},title={Text(stringResource(R.string.application_remove_title,name))},text={Text(stringResource(R.string.application_remove_preserves))},
        confirmButton={TextButton(onClick={model.removeApplication(captured.first,captured.second);removal=null}){Text(stringResource(R.string.remove_app))}},
        dismissButton={TextButton(onClick={removal=null}){Text(stringResource(R.string.cancel))}})}
}
