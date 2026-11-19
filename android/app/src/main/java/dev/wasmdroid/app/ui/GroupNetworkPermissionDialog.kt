package dev.wasmdroid.app.ui

import androidx.compose.foundation.layout.*
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.pluralStringResource
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.unit.dp
import dev.wasmdroid.app.R

@Composable internal fun GroupNetworkPermissionDialog(controller:GroupNetworkPermissionController,app:AppState,localNetwork:LocalNetworkAccess){
    val state by controller.state.collectAsState();if(!state.open)return
    val epoch=state.epoch;val source=state.source
    val memberCount = source?.members?.size ?: 0
    val current=app.connected&&controller.available()
    val active=source?.members?.any{it.id in app.controllers}==true||source?.canEdit==false
    val enabled=current&&!state.busy&&!active&&source?.supported==true
    AlertDialog(onDismissRequest={controller.dismiss(epoch)},title={Text(stringResource(R.string.network_permission_title))},text={
        Column(Modifier.fillMaxWidth().verticalScroll(rememberScrollState()),verticalArrangement=Arrangement.spacedBy(12.dp)){
            source?.let{
                Text(it.name,style=MaterialTheme.typography.titleMedium)
                Text(pluralStringResource(R.plurals.group_network_services, memberCount, memberCount))
                it.members.forEach{member->Text(member.service)}
            }
            if(state.loading)LinearProgressIndicator(Modifier.fillMaxWidth())
            else{
                val problem=when{
                    !current->R.string.network_permission_unavailable
                    active->R.string.group_network_stop
                    source?.supported!=true->R.string.network_permission_unsupported
                    state.problem!=null->R.string.network_permission_changed
                    else->null
                }
                problem?.let{Text(stringResource(it),color=MaterialTheme.colorScheme.error)}
                if(source!=null){
                    Text(stringResource(R.string.network_permission_intro))
                    GroupPermissionChoice(R.string.network_permission_internet,R.string.network_permission_internet_body,state.internet,enabled,source.networkEnabled){controller.choose(epoch,true,it)}
                    GroupPermissionChoice(R.string.network_permission_lan,R.string.network_permission_lan_body,state.lan,enabled,source.networkEnabled){controller.choose(epoch,false,it)}
                    if(enabled&&source.networkEnabled&&state.lan==true&&!localNetwork.granted){
                        LocalNetworkAccessActions(localNetwork,R.string.network_permission_android_lan)
                    }
                    if(!source.networkEnabled)Text(stringResource(R.string.network_permission_disabled))
                    Text(stringResource(R.string.group_network_new_services))
                    Text(stringResource(R.string.network_permission_browser),style=MaterialTheme.typography.bodySmall)
                    Text(stringResource(R.string.network_permission_route),style=MaterialTheme.typography.bodySmall)
                    Text(stringResource(R.string.network_permission_next_run))
                }
                if(!state.saving&&(!current||state.problem!=null))TextButton(onClick={controller.reload(epoch)}){Text(stringResource(R.string.reload))}
                if(state.saving)LinearProgressIndicator(Modifier.fillMaxWidth())
            }
        }
    },confirmButton={Button(
        enabled=enabled&&state.internet!=null&&state.lan!=null&&(source?.networkEnabled==true||(state.internet==false&&state.lan==false))&&
            (state.lan!=true||localNetwork.granted),
        onClick={if(state.lan!=true||localNetwork.refresh())controller.save(epoch)}
    ){
        Text(pluralStringResource(R.plurals.group_network_save, memberCount, memberCount))
    }},dismissButton={TextButton(enabled=!state.saving,onClick={controller.dismiss(epoch)}){Text(stringResource(R.string.cancel))}})
}
@Composable private fun GroupPermissionChoice(title:Int,body:Int,value:Boolean?,enabled:Boolean,canGrant:Boolean,choose:(Boolean)->Unit){
    Column(verticalArrangement=Arrangement.spacedBy(4.dp)){
        Text(stringResource(title),style=MaterialTheme.typography.titleMedium)
        Text(stringResource(body),style=MaterialTheme.typography.bodySmall)
        if(value==null)Text(stringResource(R.string.group_network_mixed))
        Row(horizontalArrangement=Arrangement.spacedBy(8.dp)){
            FilterChip(selected=value==false,onClick={choose(false)},enabled=enabled,label={Text(stringResource(R.string.group_network_off))})
            FilterChip(selected=value==true,onClick={choose(true)},enabled=enabled&&canGrant,label={Text(stringResource(R.string.group_network_allow))})
        }
    }
}
