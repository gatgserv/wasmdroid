package dev.wasmdroid.app.ui

import androidx.compose.foundation.layout.*
import androidx.compose.material3.*
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.LiveRegionMode
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.liveRegion
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import dev.wasmdroid.app.R

/** Visible, read-only Settings observation. There is no background polling or
 * invented total-app/physical-allocation/quota control in this pane. */
@Composable internal fun StorageUsagePane(state:StorageUsageState,connected:Boolean,onRefresh:()->Unit) {
    LaunchedEffect(connected){if(connected)onRefresh()}
    val phase=if(connected)state.phase else StorageUsagePhase.Unavailable
    val snapshot=state.snapshot.takeIf{phase==StorageUsagePhase.Ready}
    OutlinedCard(Modifier.fillMaxWidth()){
        Column(Modifier.fillMaxWidth().padding(16.dp),verticalArrangement=Arrangement.spacedBy(12.dp)){
            Text(stringResource(R.string.storage_title),style=MaterialTheme.typography.titleMedium,
                modifier=Modifier.semantics{heading()})
            Text(stringResource(R.string.storage_scope),style=MaterialTheme.typography.bodyMedium)
            Text(stringResource(when(phase){
                StorageUsagePhase.Idle->R.string.storage_idle
                StorageUsagePhase.Loading->R.string.storage_loading
                StorageUsagePhase.Failed->R.string.storage_failed
                StorageUsagePhase.Unavailable->R.string.storage_disconnected
                StorageUsagePhase.Ready->if(snapshot?.complete==true)R.string.storage_counted else R.string.storage_partial
            }),style=MaterialTheme.typography.labelLarge,
                modifier=Modifier.semantics{liveRegion=LiveRegionMode.Polite})
            if(phase==StorageUsagePhase.Loading)LinearProgressIndicator(Modifier.fillMaxWidth())
            if(snapshot!=null){
                snapshot.categories.forEach{category->
                    Column(Modifier.fillMaxWidth(),verticalArrangement=Arrangement.spacedBy(3.dp)){
                        Row(Modifier.fillMaxWidth(),horizontalArrangement=Arrangement.spacedBy(12.dp)){
                            Text(stringResource(when(category.category){
                                StorageCategory.Artifacts->R.string.storage_artifacts
                                StorageCategory.CompiledCache->R.string.storage_compiled_cache
                                StorageCategory.Volumes->R.string.storage_volumes
                                StorageCategory.Logs->R.string.storage_logs
                            }),style=MaterialTheme.typography.labelLarge,modifier=Modifier.weight(1f))
                            Text(category.bytes?.let(::formatStorageBytes)?:stringResource(R.string.storage_value_unavailable),
                                modifier=Modifier.weight(1f),textAlign=TextAlign.End)
                        }
                        val detail=when(category.status){
                            StorageCountStatus.Complete->null
                            StorageCountStatus.Partial->R.string.storage_row_partial
                            StorageCountStatus.Unavailable->R.string.storage_row_unavailable
                        }
                        detail?.let{Text(stringResource(it),style=MaterialTheme.typography.bodySmall)}
                    }
                }
                HorizontalDivider()
                Column(Modifier.fillMaxWidth(),verticalArrangement=Arrangement.spacedBy(3.dp)){
                    Text(stringResource(R.string.storage_observed_sum),style=MaterialTheme.typography.labelLarge)
                    Text(snapshot.observedTotalBytes?.let(::formatStorageBytes)?:stringResource(R.string.storage_value_unavailable))
                    if(!snapshot.complete)Text(stringResource(R.string.storage_sum_partial),style=MaterialTheme.typography.bodySmall)
                }
                Column(Modifier.fillMaxWidth(),verticalArrangement=Arrangement.spacedBy(3.dp)){
                    Text(stringResource(R.string.storage_available),style=MaterialTheme.typography.labelLarge)
                    Text(snapshot.availableBytes?.let(::formatStorageBytes)?:stringResource(R.string.storage_value_unavailable))
                    Text(stringResource(R.string.storage_available_detail),style=MaterialTheme.typography.bodySmall)
                }
            }
            if(snapshot?.complete!=false)Text(stringResource(R.string.storage_observation_detail),style=MaterialTheme.typography.bodySmall)
            Text(stringResource(R.string.storage_exclusions),style=MaterialTheme.typography.bodySmall)
            OutlinedButton(enabled=connected&&phase!=StorageUsagePhase.Loading,onClick=onRefresh){
                Text(stringResource(R.string.storage_refresh))
            }
            if(phase==StorageUsagePhase.Failed)DiagnosticReportButton(
                dev.wasmdroid.app.reports.ReportIssue.fromCodes("other","data","failed"))
        }
    }
}
