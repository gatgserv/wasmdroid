package dev.wasmdroid.app.ui

import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.material3.*
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.platform.LocalWindowInfo
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.LiveRegionMode
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.liveRegion
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.dp
import dev.wasmdroid.app.R

@Composable internal fun PackageOperationsPane(state:AppState,model:AppModel,downloadedPackages:Map<String,String>){
    if(state.packages.isEmpty()){
        // Native registration may still be queued. Do not offer cancellation
        // until an actual current generation is available.
        if(state.busy.any{it=="install"||it.startsWith("pull:")||it.startsWith("package:retry:")})OperationProgress(null)
        return
    }
    // Keep the application destination usable even with retained outcomes or
    // several requests. The native lazy list also supports large-text scrolling.
    val windowHeight = LocalWindowInfo.current.containerSize.height
    val height = with(LocalDensity.current) { maxOf(windowHeight.toDp() * 0.45f, 120.dp) }
    LazyColumn(Modifier.fillMaxWidth().heightIn(max=height),contentPadding=PaddingValues(horizontal=20.dp,vertical=8.dp),verticalArrangement=Arrangement.spacedBy(8.dp)){
        if(state.packagesUnavailable||!state.connected)item{Text(stringResource(R.string.package_unavailable))}
        items(state.packages,key={it.identity}){task->
            val downloaded=task.downloadedRegistryReference?.let(downloadedPackages::get)
            PackageOperationCard(task,state,model,downloaded)
        }
    }
}

@OptIn(ExperimentalLayoutApi::class)
@Composable private fun PackageOperationCard(task:PackageTask,state:AppState,model:AppModel,downloaded:String?){
    val cancelling=!task.terminal&&(task.status=="cancelled"||task.identity in state.packageCancels)
    val name=when {
        task.kind=="local_import"->stringResource(R.string.local_package_import)
        task.kind=="image"&&task.label.startsWith("wasmer.io/")->task.label.removePrefix("wasmer.io/")
        task.label.isBlank()||(!state.advanced&&task.label.startsWith("wasmer.io/"))->stringResource(R.string.package_installation)
        else->task.label
    }
    val usable=state.connected&&!state.packagesUnavailable
    val busy=state.busy.any{it=="package:retry:${task.identity}"||it=="package:dismiss:${task.identity}"}
    val phase=when{
        cancelling->R.string.package_cancelling
        task.terminal&&task.status=="installed"->when(task.kind){
            "application"->R.string.package_app_ready
            "image"->R.string.downloaded
            else->R.string.installed
        }
        task.terminal&&task.status=="cancelled"->R.string.package_cancelled
        task.terminal->R.string.package_failed
        task.status=="preparing_app"->R.string.package_preparing_app
        task.status=="awaiting_review"->R.string.local_package_review
        task.status in setOf("publishing","committed","failed")->R.string.package_finalizing
        task.phase=="downloading"->R.string.downloading
        task.phase=="verifying"->R.string.verifying
        task.phase=="reading_local"->R.string.local_package_reading
        task.phase=="inspecting_local"->R.string.local_package_inspecting
        task.phase=="installing_local"->R.string.verifying
        else->R.string.preparing
    }
    OutlinedCard(Modifier.fillMaxWidth()){
        Column(Modifier.padding(16.dp),verticalArrangement=Arrangement.spacedBy(8.dp)){
            Text(name,style=MaterialTheme.typography.titleMedium)
            Text(stringResource(phase),style=MaterialTheme.typography.labelLarge,modifier=Modifier.semantics{liveRegion=LiveRegionMode.Polite})
            if(state.advanced)task.reference?.takeIf{it.isNotBlank()}?.let{Text(it,style=MaterialTheme.typography.bodySmall)}
            if(!task.terminal){
                if(task.status=="awaiting_review"&&!cancelling)Text(stringResource(R.string.local_package_review_detail),style=MaterialTheme.typography.bodySmall)
                else if(task.total>0&&task.status=="active"&&!cancelling){
                    Text(stringResource(R.string.package_current_bytes,task.bytes/1024,task.total/1024),style=MaterialTheme.typography.bodySmall)
                    LinearProgressIndicator(progress={(task.bytes.toFloat()/task.total).coerceIn(0f,1f)},modifier=Modifier.fillMaxWidth())
                }else LinearProgressIndicator(Modifier.fillMaxWidth())
                if(cancelling)Text(stringResource(R.string.package_cancelling_detail),style=MaterialTheme.typography.bodySmall)
                else if(task.status in setOf("publishing","committed","preparing_app","failed"))Text(stringResource(R.string.package_finishing_detail),style=MaterialTheme.typography.bodySmall)
            }else{
                if(task.status=="cancelled")Text(stringResource(R.string.package_cancelled_detail),style=MaterialTheme.typography.bodySmall)
                else if(task.status=="installed")Text(stringResource(when(task.kind){"application"->R.string.package_app_ready_detail;"local_import"->R.string.local_package_create_separate;else->R.string.package_image_ready_detail}),style=MaterialTheme.typography.bodySmall)
                else{
                    if(task.imageCommitted&&task.kind=="application")Text(stringResource(R.string.package_setup_failed),color=MaterialTheme.colorScheme.error,style=MaterialTheme.typography.bodySmall)
                    Text(
                        stringResource(when(task.errorKind){
                            "space" -> R.string.error_space
                            "busy" -> R.string.package_busy
                            "integrity" -> R.string.error_integrity
                            "registry_webc_version" -> R.string.package_webc_v3_required
                            "unsupported" -> R.string.error_unsupported
                            "network" -> R.string.error_network
                            "local_input" -> R.string.local_package_invalid
                            else -> R.string.error_generic
                        }),
                        color=MaterialTheme.colorScheme.error,
                        style=MaterialTheme.typography.bodySmall,
                    )
                    if(state.advanced)task.technicalError?.let{Text(it,style=MaterialTheme.typography.bodySmall)}
                }
            }
            FlowRow(horizontalArrangement=Arrangement.spacedBy(8.dp),verticalArrangement=Arrangement.spacedBy(4.dp)){
                if(downloaded!=null)CreateDownloadedPackageServiceButton(downloaded,state,model)
                if(task.kind=="local_import")Button(enabled=usable&&!busy&&!cancelling,onClick={model.openLocalPackage(task)}){Text(stringResource(when(task.status){"installed"->R.string.create_service;"awaiting_review"->R.string.local_package_review;else->R.string.local_package_details}))}
                if(task.cancellable&&!cancelling){
                    val cancelLabel=stringResource(R.string.package_cancel_named,name)
                    OutlinedButton(enabled=usable&&!busy,onClick={model.cancelPackage(task)},modifier=Modifier.semantics{contentDescription=cancelLabel}){Text(stringResource(R.string.package_cancel))}
                }
                if(task.retryable)Button(enabled=usable&&!busy&&!state.packageWorkActive()&&(task.kind!="application"||state.runtimeAvailable),onClick={model.retryPackage(task)}){Text(stringResource(R.string.retry))}
                if(task.terminal&&task.status=="failed")DiagnosticReportButton(dev.wasmdroid.app.reports.ReportIssue.fromCodes(task.errorKind,task.kind,task.phase?:task.status))
                if(task.terminal)TextButton(enabled=usable&&!busy,onClick={model.dismissPackage(task)}){Text(stringResource(R.string.dismiss))}
            }
        }
    }
}
