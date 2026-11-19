package dev.wasmdroid.app.ui

import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.foundation.selection.toggleable
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.runtime.saveable.LocalSaveableStateRegistry
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.LiveRegionMode
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.liveRegion
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.text.input.PasswordVisualTransformation
import androidx.compose.ui.text.input.VisualTransformation
import androidx.compose.ui.unit.dp
import androidx.compose.ui.window.Dialog
import androidx.compose.ui.window.DialogProperties
import dev.wasmdroid.app.R
import dev.wasmdroid.app.engine.PackageRuntimeRequirements
import dev.wasmdroid.app.engine.PackageRuntimeRequirements.Kind
import org.json.JSONObject

@OptIn(ExperimentalMaterial3Api::class,ExperimentalLayoutApi::class)
@Composable internal fun ServiceConfigurationScreen(controller:ServiceConfigurationController,app:AppState){
    val state by controller.state.collectAsState()
    if(!state.open)return
    val epoch=state.epoch;val draft=state.draft
    val record=remember(app.instances,state.instanceId){app.instances.asSequence().map(::JSONObject).firstOrNull{it.getString("id")==state.instanceId}}
    val active=state.instanceId!=null&&(state.instanceId in app.controllers||
        (record?.getString("state")?:state.sourceState) in setOf("running","paused","restarting"))
    val stale=record!=null&&state.version!=null&&record.getLong("version").toULong()!=state.version
    val managed=state.groupId!=null||(state.instanceId!=null&&state.instanceId in app.memberships)
    val current=app.connected&&controller.available()
    val enabled=!state.busy&&!managed
    var showValues by remember(epoch,draft?.environment?.size){mutableStateOf(false)}
    val artifact = remember(app.images, state.reference) {
        app.images.asSequence().map(::JSONObject).firstOrNull { it.optString("reference") == state.reference }
    }
    val requiredSettings = PackageRuntimeRequirements.settings(
        artifact, draft?.command.orEmpty(),
        draft?.environment.orEmpty().filter { it.required }.map { it.name }.toSet(),
    )
    val commands=remember(artifact,draft?.command){
        val installed=artifact?.optJSONArray("commands")
        (listOfNotNull(draft?.command?.takeIf{it.isNotBlank()})+(0 until (installed?.length()?:0)).take(256).map{installed!!.getString(it)}).distinct()
    }
    val volumes=remember(app.volumes){app.volumes.map{JSONObject(it).getString("name")}.sorted()}
    Dialog(onDismissRequest={controller.dismiss(epoch)},properties=DialogProperties(usePlatformDefaultWidth=false)){
        CompositionLocalProvider(LocalSaveableStateRegistry provides null){
        Surface(Modifier.fillMaxSize()){
            Column(Modifier.fillMaxSize().safeDrawingPadding().imePadding()){
                Row(Modifier.fillMaxWidth().padding(horizontal=12.dp,vertical=8.dp),horizontalArrangement=Arrangement.spacedBy(8.dp)){
                    TextButton(enabled=!state.saving&&!state.makingVolume,onClick={controller.dismiss(epoch)}){Text(stringResource(R.string.back))}
                    Text(stringResource(if(state.instanceId==null)R.string.config_create_title else R.string.config_edit_title),style=MaterialTheme.typography.titleLarge,modifier=Modifier.weight(1f).padding(top=8.dp))
                }
                PrimaryScrollableTabRow(selectedTabIndex=state.tab,edgePadding=0.dp){
                    listOf(R.string.config_application,R.string.config_execution,R.string.config_data,R.string.config_json).forEachIndexed{index,label->
                        Tab(selected=state.tab==index,enabled=!state.busy,onClick={controller.tab(epoch,index)},text={Text(stringResource(label))})
                    }
                }
                LazyColumn(Modifier.weight(1f).fillMaxWidth(),contentPadding=PaddingValues(20.dp),verticalArrangement=Arrangement.spacedBy(12.dp)){
                    item{
                        if(state.busy){Text(stringResource(when{state.saving->R.string.config_saving;state.makingVolume->R.string.config_volume_creating;else->R.string.preparing}));LinearProgressIndicator(Modifier.fillMaxWidth())}
                        else if(!current)Text(stringResource(R.string.config_unavailable),color=MaterialTheme.colorScheme.error)
                        else if(stale)Text(stringResource(R.string.config_changed),color=MaterialTheme.colorScheme.error)
                        else if(active)Text(stringResource(R.string.config_stop_first),color=MaterialTheme.colorScheme.error)
                        if(managed)Text(stringResource(R.string.automation_group_unavailable),color=MaterialTheme.colorScheme.error)
                        if(!app.runtimeAvailable)Text(stringResource(R.string.runtime_missing))
                        state.problem?.let{Text(stringResource(problemText(state)),color=MaterialTheme.colorScheme.error,modifier=Modifier.semantics{liveRegion=LiveRegionMode.Polite})}
                        if(!state.busy&&(stale||!current||state.problem==ConfigurationProblem.Changed)){
                            Text(stringResource(R.string.config_reload_hint),style=MaterialTheme.typography.bodySmall)
                            TextButton(onClick={controller.reload(epoch)}){Text(stringResource(R.string.reload))}
                        }
                    }
                    if(state.tab==3){
                        item{Text(stringResource(R.string.config_json_private),style=MaterialTheme.typography.bodySmall)}
                        item{OutlinedTextField(value=state.raw,onValueChange={controller.raw(epoch,it)},enabled=enabled,modifier=Modifier.fillMaxWidth(),
                            label={Text(stringResource(R.string.config_json))},minLines=10,maxLines=18,isError=state.problem==ConfigurationProblem.Input,
                            textStyle=MaterialTheme.typography.bodySmall.copy(fontFamily=FontFamily.Monospace))}
                    }else if(draft!=null)when(state.tab){
                        0->{
                            item{ConfigField(R.string.config_name,draft.name,enabled,state.field==ConfigurationField.Name){value->controller.change(epoch){it.copy(name=value)}}}
                            item{Text(state.reference,style=MaterialTheme.typography.bodySmall)}
                            item{ChoiceField(R.string.config_command,draft.command,commands,enabled){value->controller.change(epoch){it.copy(command=value)}}}
                            item{ConfigField(R.string.config_working_directory,draft.workingDirectory,enabled,state.field==ConfigurationField.WorkingDirectory){value->controller.change(epoch){it.copy(workingDirectory=value)}}}
                            item{ConfigField(R.string.config_timeout,draft.timeoutSeconds,enabled,state.field==ConfigurationField.Timeout,KeyboardType.Number){value->controller.change(epoch){it.copy(timeoutSeconds=value)}}}
                            item{ConfigCheck(R.string.terminal_config_tty,draft.tty,enabled){value->controller.change(epoch){it.copy(tty=value)}}}
                            item{ConfigCheck(R.string.terminal_config_stdin,draft.stdinOpen,enabled){value->controller.change(epoch){it.copy(stdinOpen=value)}}}
                            item{HorizontalDivider();Text(stringResource(R.string.automation_title),style=MaterialTheme.typography.titleMedium,modifier=Modifier.semantics{heading()});AutomaticStartControls(draft.startOnBoot,draft.restartPolicy,enabled,
                                {value->controller.change(epoch){it.copy(startOnBoot=value)}},
                                {value->controller.change(epoch){it.copy(restartPolicy=value)}})}
                        }
                        1->{
                            if (requiredSettings.isNotEmpty()) {
                                item {
                                    Text(stringResource(R.string.config_required_title), style = MaterialTheme.typography.titleMedium)
                                    Text(stringResource(R.string.config_required_help), style = MaterialTheme.typography.bodySmall)
                                }
                                requiredSettings.forEach { setting ->
                                    item(key = "required-${setting.variable}") {
                                        val value = draft.environment.firstOrNull { it.name == setting.variable }?.value.orEmpty()
                                        val secret = setting.kind == Kind.Secret
                                        ConfigField(setting.variable, value, enabled,
                                            value.isNotEmpty() && !setting.accepts(value),
                                            when (setting.kind) {
                                                Kind.Secret -> KeyboardType.Password
                                                Kind.Port -> KeyboardType.Number
                                                Kind.Text -> KeyboardType.Text
                                            }, secret) { entered ->
                                            controller.change(epoch) { current ->
                                                val prior = current.environment.firstOrNull { it.name == setting.variable }
                                                val other = current.environment.filterNot { it.name == setting.variable }
                                                val required = prior?.required ?: setting.manual
                                                current.copy(environment = when {
                                                    entered.isEmpty() && !required -> other
                                                    prior == null && other.size >= 128 -> current.environment
                                                    else -> other + EnvironmentDraft(setting.variable, entered, required)
                                                })
                                            }
                                        }
                                        if (setting.manual) {
                                            ConfigCheck(R.string.config_required_before_start, true, enabled) { checked ->
                                                controller.change(epoch) { current ->
                                                    current.copy(environment = current.environment.map {
                                                        if (it.name == setting.variable) it.copy(required = checked) else it
                                                    })
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                            item{Text(stringResource(R.string.config_arguments),style=MaterialTheme.typography.titleMedium);Text(stringResource(R.string.config_argument_hint),style=MaterialTheme.typography.bodySmall)}
                            items(draft.arguments.size,key={"argument_${draft.arguments.size}_$it"}){index->
                                ConfigRow(stringResource(R.string.config_argument_number,index+1),enabled,{controller.change(epoch){it.copy(arguments=it.arguments.filterIndexed{i,_->i!=index})}}){
                                    ConfigField(R.string.config_argument_value,draft.arguments[index],enabled,state.field==ConfigurationField.Arguments){value->controller.change(epoch){it.copy(arguments=it.arguments.mapIndexed{i,old->if(i==index)value else old})}}
                                }
                            }
                            item{OutlinedButton(enabled=enabled&&draft.arguments.size<256,onClick={controller.change(epoch){it.copy(arguments=it.arguments+"")}}){Text(stringResource(R.string.config_add_argument))}}
                            item{HorizontalDivider();Text(stringResource(R.string.config_environment),style=MaterialTheme.typography.titleMedium);ConfigCheck(R.string.config_show_values,showValues,true){showValues=it}}
                            val otherEnvironment = draft.environment.withIndex().filter { entry ->
                                requiredSettings.none { it.variable == entry.value.name }
                            }
                            items(otherEnvironment.size,key={ "environment_${otherEnvironment.size}_$it" }){position->
                                val (index, row) = otherEnvironment[position]
                                ConfigRow(stringResource(R.string.config_environment_number,position+1),enabled,{controller.change(epoch){it.copy(environment=it.environment.filterIndexed{i,_->i!=index})}}){
                                    ConfigField(R.string.config_environment_name,row.name,enabled,state.field==ConfigurationField.Environment){value->controller.change(epoch){it.copy(environment=it.environment.mapIndexed{i,old->if(i==index)old.copy(name=value)else old})}}
                                    ConfigField(R.string.config_environment_value,row.value,enabled,state.field==ConfigurationField.Environment,
                                        if(showValues)KeyboardType.Text else KeyboardType.Password,!showValues){value->controller.change(epoch){it.copy(environment=it.environment.mapIndexed{i,old->if(i==index)old.copy(value=value)else old})}}
                                    ConfigCheck(R.string.config_required_before_start, row.required,
                                        enabled && PackageRuntimeRequirements.validVariable(row.name)) { required ->
                                        controller.change(epoch) { current ->
                                            current.copy(environment = current.environment.mapIndexed { i, entry ->
                                                if (i == index) entry.copy(required = required) else entry
                                            })
                                        }
                                    }
                                }
                            }
                            item{OutlinedButton(enabled=enabled&&draft.environment.size<128,onClick={controller.change(epoch){it.copy(environment=it.environment+EnvironmentDraft("",""))}}){Text(stringResource(R.string.config_add_environment))}}
                        }
                        2->{
                            item{Text(stringResource(R.string.config_volumes),style=MaterialTheme.typography.titleMedium);Text(stringResource(R.string.config_volume_preserved),style=MaterialTheme.typography.bodySmall)}
                            items(draft.mounts.size,key={"mount_${draft.mounts.size}_$it"}){index->
                                val row=draft.mounts[index]
                                ConfigRow(stringResource(R.string.config_mount_number,index+1),enabled,{controller.change(epoch){it.copy(mounts=it.mounts.filterIndexed{i,_->i!=index})}},R.string.config_disconnect){
                                    ChoiceField(R.string.config_volume,row.volume,(listOf(row.volume)+volumes).distinct(),enabled){value->controller.change(epoch){it.copy(mounts=it.mounts.mapIndexed{i,old->if(i==index)old.copy(volume=value)else old})}}
                                    ConfigField(R.string.config_mount_target,row.target,enabled,state.field==ConfigurationField.Mounts){value->controller.change(epoch){it.copy(mounts=it.mounts.mapIndexed{i,old->if(i==index)old.copy(target=value)else old})}}
                                    ConfigCheck(R.string.config_read_only,row.readOnly,enabled){value->controller.change(epoch){it.copy(mounts=it.mounts.mapIndexed{i,old->if(i==index)old.copy(readOnly=value)else old})}}
                                }
                            }
                            item{OutlinedButton(enabled=enabled&&volumes.isNotEmpty()&&draft.mounts.size<32,onClick={controller.change(epoch){it.copy(mounts=it.mounts+MountDraft(volumes.first(),"/data",false))}}){Text(stringResource(R.string.config_connect_volume))}}
                            item{ConfigField(R.string.config_new_volume,state.volumeName,enabled,false){controller.volumeName(epoch,it)}
                                OutlinedButton(enabled=enabled&&current&&state.volumeName.isNotBlank()&&draft.mounts.size<32,onClick={controller.createVolume(epoch)}){Text(stringResource(R.string.config_create_volume))}
                                Text(stringResource(R.string.config_new_volume_hint),style=MaterialTheme.typography.bodySmall)}
                            item{HorizontalDivider();ChoiceField(R.string.config_network,draft.network,listOf("loopback","none"),enabled,
                                {if(it=="none")stringResource(R.string.group_network_none)else stringResource(R.string.this_phone)}){value->controller.change(epoch){it.copy(network=value)}}}
                            item{Text(stringResource(R.string.config_ports),style=MaterialTheme.typography.titleMedium);Text(stringResource(R.string.config_ports_hint),style=MaterialTheme.typography.bodySmall)}
                            items(draft.ports.size,key={"port_${draft.ports.size}_$it"}){index->
                                val row=draft.ports[index]
                                ConfigRow(stringResource(R.string.config_port_number,index+1),enabled,{controller.change(epoch){it.copy(ports=it.ports.filterIndexed{i,_->i!=index})}}){
                                    ConfigField(R.string.config_guest_port,row.guest,enabled,state.field==ConfigurationField.Ports,KeyboardType.Number){value->controller.change(epoch){it.copy(ports=it.ports.mapIndexed{i,old->if(i==index)old.copy(guest=value)else old})}}
                                    ConfigCheck(R.string.config_startup_port, row.startup, enabled) { selected ->
                                        controller.change(epoch) { current ->
                                            current.copy(ports = current.ports.mapIndexed { i, port ->
                                                if (i == index || selected) port.copy(startup = i == index && selected) else port
                                            })
                                        }
                                    }
                                    if (row.startup) {
                                        Text(stringResource(R.string.config_startup_port_hint), style = MaterialTheme.typography.bodySmall)
                                    }
                                    ConfigCheck(R.string.config_automatic_port,row.phone=="0",enabled){value->controller.change(epoch){it.copy(ports=it.ports.mapIndexed{i,old->if(i==index)old.copy(phone=if(value)"0" else "")else old})}}
                                    if(row.phone!="0")ConfigField(R.string.config_phone_port,row.phone,enabled,state.field==ConfigurationField.Ports,KeyboardType.Number){value->controller.change(epoch){it.copy(ports=it.ports.mapIndexed{i,old->if(i==index)old.copy(phone=value)else old})}}
                                    ConfigCheck(R.string.config_web_port, row.web, enabled) { selected ->
                                        controller.change(epoch) { current ->
                                            current.copy(ports = current.ports.mapIndexed { i, port ->
                                                if (i == index || selected) port.copy(web = i == index && selected) else port
                                            })
                                        }
                                    }
                                }
                            }
                            item {
                                OutlinedButton(enabled = enabled && draft.ports.size < 16, onClick = {
                                    controller.change(epoch) { current ->
                                        current.copy(ports = current.ports + PortDraft("", web = current.ports.isEmpty()))
                                    }
                                }) { Text(stringResource(R.string.config_add_port)) }
                            }
                        }
                    }
                }
                Column(Modifier.fillMaxWidth().padding(horizontal=20.dp,vertical=10.dp)){
                    Text(stringResource(if(state.instanceId==null)R.string.local_package_no_start else R.string.config_next_start),style=MaterialTheme.typography.bodySmall)
                    FlowRow(Modifier.fillMaxWidth(),horizontalArrangement=Arrangement.spacedBy(8.dp)){
                        TextButton(enabled=!state.saving&&!state.makingVolume,onClick={controller.dismiss(epoch)}){Text(stringResource(R.string.cancel))}
                        Button(enabled=enabled&&current&&app.runtimeAvailable&&!active&&!stale&&state.problem!=ConfigurationProblem.Changed,onClick={controller.save(epoch)}){Text(stringResource(if(state.instanceId==null)R.string.create_service else R.string.config_save))}
                    }
                }
            }
        }
        }
    }
}

@Composable private fun ConfigField(label:Int,value:String,enabled:Boolean,error:Boolean,keyboard:KeyboardType=KeyboardType.Text,secret:Boolean=false,changed:(String)->Unit){
    ConfigField(stringResource(label), value, enabled, error, keyboard, secret, changed)
}
@Composable private fun ConfigField(label:String,value:String,enabled:Boolean,error:Boolean,keyboard:KeyboardType=KeyboardType.Text,secret:Boolean=false,changed:(String)->Unit){
    OutlinedTextField(value=value,onValueChange=changed,enabled=enabled,isError=error,label={Text(label)},modifier=Modifier.fillMaxWidth(),
        keyboardOptions=KeyboardOptions(keyboardType=keyboard),visualTransformation=if(secret)PasswordVisualTransformation()else VisualTransformation.None,
        maxLines=if(secret)1 else 4)
}
@Composable private fun ConfigCheck(label: Int, value: Boolean, enabled: Boolean, changed: (Boolean) -> Unit) {
    Row(
        modifier = Modifier
            .fillMaxWidth()
            .heightIn(min = 48.dp)
            .toggleable(value = value, enabled = enabled, role = Role.Checkbox, onValueChange = changed),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(12.dp),
    ) {
        Checkbox(checked = value, enabled = enabled, onCheckedChange = null)
        Text(stringResource(label))
    }
}
@Composable private fun ChoiceField(label:Int,value:String,choices:List<String>,enabled:Boolean,text: @Composable (String)->String={it},changed:(String)->Unit){
    var expanded by remember{mutableStateOf(false)}
    Column{
        Text(stringResource(label),style=MaterialTheme.typography.labelMedium)
        OutlinedButton(enabled=enabled&&choices.isNotEmpty(),onClick={expanded=true}){Text(text(value))}
        DropdownMenu(expanded=expanded,onDismissRequest={expanded=false}){
            choices.forEach{choice->DropdownMenuItem(text={Text(text(choice))},onClick={expanded=false;changed(choice)})}
        }
    }
}
@Composable private fun ConfigRow(label:String,enabled:Boolean,remove:()->Unit,removeLabel:Int=R.string.config_remove_row,body:@Composable ColumnScope.()->Unit){
    Column(verticalArrangement=Arrangement.spacedBy(8.dp)){
        Text(label,style=MaterialTheme.typography.labelLarge)
        body()
        TextButton(enabled=enabled,onClick=remove){Text(stringResource(removeLabel))}
    }
}
private fun problemText(state:ConfigurationState):Int=when(state.problem){
    ConfigurationProblem.Input->when(state.field){
        ConfigurationField.Name->R.string.config_error_name;ConfigurationField.Command->R.string.config_error_command
        ConfigurationField.Arguments->R.string.config_error_arguments;ConfigurationField.Environment->R.string.config_error_environment
        ConfigurationField.WorkingDirectory->R.string.config_error_directory;ConfigurationField.Mounts->R.string.config_error_mounts
        ConfigurationField.Ports->R.string.config_error_ports;ConfigurationField.Timeout->R.string.config_error_timeout
        ConfigurationField.Network->R.string.config_error_network;ConfigurationField.Automation->R.string.automation_error
        else->R.string.config_error_document
    }
    ConfigurationProblem.Unavailable->R.string.config_unavailable;ConfigurationProblem.Changed->R.string.config_changed
    ConfigurationProblem.Unsupported->R.string.error_unsupported;ConfigurationProblem.Busy->R.string.config_stop_first
    ConfigurationProblem.Volume->R.string.config_volume_failed;ConfigurationProblem.VolumeName->R.string.config_volume_name_invalid
    else->R.string.config_save_failed
}
