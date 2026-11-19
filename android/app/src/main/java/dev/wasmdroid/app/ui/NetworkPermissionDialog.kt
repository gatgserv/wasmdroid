package dev.wasmdroid.app.ui

import androidx.compose.foundation.layout.*
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.selection.toggleable
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.unit.dp
import dev.wasmdroid.app.R
import org.json.JSONObject

@Composable internal fun NetworkPermissionDialog(controller:NetworkPermissionController,app:AppState,localNetwork:LocalNetworkAccess){
    val state by controller.state.collectAsState()
    if(!state.open)return
    val epoch=state.epoch;val source=state.source
    val record=remember(app.instances,state.id){app.instances.asSequence().map(::JSONObject).firstOrNull{it.getString("id")==state.id}}
    val changed=source!=null&&(record==null||record.getLong("version").toULong()!=source.version)
    val active=state.id?.let{it in app.controllers}==true||source?.status !in NetworkPermissionController.STOPPED||
        record?.optString("state")?.let{it !in NetworkPermissionController.STOPPED}==true
    val current=app.connected&&controller.available()
    val enabled=!state.busy&&current&&!changed&&!active&&source?.supported==true&&!source.managed
    val canGrant=enabled&&source?.networkEnabled==true
    AlertDialog(onDismissRequest={controller.dismiss(epoch)},title={Text(stringResource(R.string.network_permission_title))},text={
        Column(Modifier.fillMaxWidth().verticalScroll(rememberScrollState()),verticalArrangement=Arrangement.spacedBy(16.dp)){
            source?.let{Text(it.name,style=MaterialTheme.typography.titleMedium)}
            if(state.loading){LinearProgressIndicator(Modifier.fillMaxWidth());Text(stringResource(R.string.preparing))}
            else{
                val problem=when{
                    !current->R.string.network_permission_unavailable
                    changed||state.problem==NetworkPermissionProblem.Changed->R.string.network_permission_changed
                    active->R.string.network_permission_stop
                    source?.supported!=true||source.managed->R.string.network_permission_unsupported
                    state.problem==NetworkPermissionProblem.Save->R.string.network_permission_failed
                    state.problem==NetworkPermissionProblem.Unavailable->R.string.network_permission_unavailable
                    else->null
                }
                problem?.let{Text(stringResource(it),color=MaterialTheme.colorScheme.error)}
                if(source!=null){
                    Text(stringResource(R.string.network_permission_intro))
                    PermissionSwitch(R.string.network_permission_internet,R.string.network_permission_internet_body,state.internet,
                        enabled&&(canGrant||state.internet)){value->controller.internet(epoch,value)}
                    PermissionSwitch(R.string.network_permission_lan,R.string.network_permission_lan_body,state.lan,
                        enabled&&(canGrant||state.lan)){value->controller.lan(epoch,value)}
                    if(canGrant&&state.lan&&!localNetwork.granted){
                        LocalNetworkAccessActions(localNetwork,R.string.network_permission_android_lan)
                    }
                    if(!source.networkEnabled)Text(stringResource(R.string.network_permission_disabled))
                    Text(stringResource(R.string.network_permission_browser),style=MaterialTheme.typography.bodySmall)
                    Text(stringResource(R.string.network_permission_route),style=MaterialTheme.typography.bodySmall)
                    Text(stringResource(R.string.network_permission_next_run))
                }
                if(!state.saving&&(!current||changed||state.problem!=null))TextButton(onClick={controller.reload(epoch)}){Text(stringResource(R.string.reload))}
                if(state.saving)LinearProgressIndicator(Modifier.fillMaxWidth())
            }
        }
    },confirmButton={Button(
        enabled=enabled&&(canGrant||(!state.internet&&!state.lan))&&(!state.lan||localNetwork.granted),
        onClick={if(!state.lan||localNetwork.refresh())controller.save(epoch)}
    ){Text(stringResource(R.string.network_permission_save))}},
        dismissButton={TextButton(enabled=!state.saving,onClick={controller.dismiss(epoch)}){Text(stringResource(R.string.cancel))}})
}
@Composable private fun PermissionSwitch(title:Int,body:Int,checked:Boolean,enabled:Boolean,change:(Boolean)->Unit){
    Row(Modifier.fillMaxWidth().toggleable(value=checked,enabled=enabled,role=Role.Switch,onValueChange=change).padding(vertical=8.dp),horizontalArrangement=Arrangement.spacedBy(16.dp)){
        Column(Modifier.weight(1f),verticalArrangement=Arrangement.spacedBy(4.dp)){
            Text(stringResource(title),style=MaterialTheme.typography.titleMedium)
            Text(stringResource(body),style=MaterialTheme.typography.bodySmall)
        }
        Switch(checked=checked,onCheckedChange=null,enabled=enabled)
    }
}
