package dev.wasmdroid.app.ui

import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.selection.selectable
import androidx.compose.foundation.selection.selectableGroup
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.LiveRegionMode
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.liveRegion
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.dp
import dev.wasmdroid.app.R

@OptIn(ExperimentalLayoutApi::class)
@Composable internal fun LocalPackageImportDialog(state:AppState,model:AppModel,configure:(String)->Unit,search:(String)->Unit){
    val selection=state.localPackageSelection?:return
    val frame=state.localPackage?.takeIf{it.identity==selection}
    val task=state.packages.firstOrNull{it.key==selection.key&&it.generation==selection.generation}
    val preview=frame?.preview
    val usable=state.connected&&!state.localPackageUnavailable&&!state.packagesUnavailable
    val cancelling=task?.let{!it.terminal&&(it.status=="cancelled"||it.identity in state.packageCancels)}==true
    val ready=frame?.status=="awaiting_review"&&frame.busy==false&&usable&&!cancelling
    val installed=frame?.status=="installed"&&frame.busy==false
    var selected by rememberSaveable(selection.key,selection.generation,preview?.token){mutableStateOf(preview?.entrypoint?:preview?.commands?.singleOrNull().orEmpty())}
    val actionBusy=state.busy.any{it.startsWith("package:local:")&&it.endsWith(selection.generation)}
    AlertDialog(onDismissRequest=model::hideLocalPackage,
        title={Text(stringResource(if(installed)R.string.local_package_installed else R.string.local_package_import))},
        text={Column(Modifier.verticalScroll(rememberScrollState()),verticalArrangement=Arrangement.spacedBy(12.dp)){
            frame?.name?.takeIf{it.isNotBlank()}?.let{Text(it,style=MaterialTheme.typography.titleMedium)}
            Text(stringResource(R.string.local_package_source),style=MaterialTheme.typography.bodySmall)
            if(!usable)Text(stringResource(R.string.local_package_unavailable),color=MaterialTheme.colorScheme.error)
            else if(cancelling)Text(stringResource(R.string.package_cancelling),modifier=Modifier.semantics{liveRegion=LiveRegionMode.Polite})
            else if(frame==null||frame.busy){
                Text(stringResource(when(task?.phase){"reading_local"->R.string.local_package_reading;"installing_local"->R.string.package_finalizing;else->R.string.local_package_inspecting}),modifier=Modifier.semantics{liveRegion=LiveRegionMode.Polite})
                LinearProgressIndicator(Modifier.fillMaxWidth())
                task?.takeIf{it.total>0&&it.phase=="reading_local"}?.let{Text(stringResource(R.string.package_current_bytes,it.bytes/1024,it.total/1024))}
            }
            frame?.errorKind?.let{kind->Text(stringResource(when(kind){
                "space"->R.string.error_space;"busy"->R.string.package_busy;"unsupported"->R.string.error_unsupported
                else->if(frame.status=="awaiting_review")R.string.local_package_review_again else R.string.local_package_invalid
            }),color=MaterialTheme.colorScheme.error)}
            if(frame?.status=="cancelled"&&!frame.busy)Text(stringResource(R.string.package_cancelled_detail))
            if(preview!=null){
                Text(stringResource(R.string.local_package_format,if(preview.kind=="wasm")"Wasm" else "WebC",preview.bytes/1024),style=MaterialTheme.typography.bodySmall)
                if(state.advanced)Text(preview.digest,style=MaterialTheme.typography.bodySmall)
                Text(stringResource(R.string.local_package_commands),style=MaterialTheme.typography.labelLarge)
                LazyColumn(Modifier.fillMaxWidth().heightIn(max=192.dp).selectableGroup()){
                items(preview.commands,key={it}){command->
                    Row(Modifier.fillMaxWidth().selectable(selected==command,enabled=ready||installed,role=Role.RadioButton,onClick={selected=command}).padding(vertical=4.dp)){
                        RadioButton(selected==command,onClick=null,enabled=ready||installed)
                        Text(command,Modifier.padding(start=10.dp,top=12.dp))
                    }
                }
                }
                if(preview.missing.isEmpty())Text(stringResource(R.string.local_package_dependencies,preview.pinCount))
                else{
                    Text(stringResource(R.string.local_package_missing),color=MaterialTheme.colorScheme.error)
                    preview.missing.forEach{dependency->
                        Text(dependency.text,style=MaterialTheme.typography.bodySmall)
                        dependency.query?.let{query->TextButton(enabled=ready&&!actionBusy,onClick={search(query)}){Text(stringResource(R.string.local_package_search_dependency))}}
                    }
                }
                Text(stringResource(R.string.local_package_no_start))
                if(installed)Text(stringResource(R.string.local_package_create_separate))
            }
        }},
        confirmButton={FlowRow(horizontalArrangement=Arrangement.spacedBy(8.dp),verticalArrangement=Arrangement.spacedBy(4.dp)){
            if(installed&&preview!=null)Button(enabled=usable&&state.runtimeAvailable&&selected in preview.commands,onClick={configure(preview.spec(selected))}){Text(stringResource(R.string.create_service))}
            else if(ready&&preview!=null&&preview.missing.isEmpty())Button(enabled=!actionBusy&&!state.packageWorkActive(),onClick={model.commitLocalPackage(preview.token)}){Text(stringResource(R.string.local_package_install))}
            if(ready)TextButton(enabled=!actionBusy,onClick=model::reviewLocalPackage){Text(stringResource(R.string.local_package_check_again))}
        }},
        dismissButton={FlowRow(horizontalArrangement=Arrangement.spacedBy(8.dp)){
            if(task?.cancellable==true&&!cancelling)TextButton(enabled=state.connected&&!state.packagesUnavailable,onClick={model.cancelPackage(task)}){Text(stringResource(R.string.cancel))}
            TextButton(onClick=model::hideLocalPackage){Text(stringResource(R.string.close))}
        }})
}
