package dev.wasmdroid.app.ui

import androidx.compose.foundation.layout.*
import androidx.compose.foundation.selection.selectable
import androidx.compose.foundation.selection.selectableGroup
import androidx.compose.foundation.selection.toggleable
import androidx.compose.material3.*
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.stateDescription
import androidx.compose.ui.unit.dp
import dev.wasmdroid.app.R

/** Only private draft changes. The existing versioned Save remains the sole
 * mutation path; no switch or selection directly starts or authorizes a run. */
@Composable internal fun AutomaticStartControls(
    startOnBoot:Boolean,
    restartPolicy:AutomaticRestartPolicy,
    enabled:Boolean,
    bootChanged:(Boolean)->Unit,
    policyChanged:(AutomaticRestartPolicy)->Unit,
) {
    val bootState=stringResource(if(startOnBoot)R.string.automation_on else R.string.automation_off)
    Column(Modifier.fillMaxWidth(),verticalArrangement=Arrangement.spacedBy(8.dp)){
        Row(Modifier.fillMaxWidth().heightIn(min=48.dp)
            .toggleable(startOnBoot,enabled=enabled,role=Role.Switch,onValueChange=bootChanged)
            .semantics{stateDescription=bootState},verticalAlignment=Alignment.CenterVertically){
            Text(stringResource(R.string.automation_after_restart),Modifier.weight(1f))
            Switch(checked=startOnBoot,onCheckedChange=null,enabled=enabled)
        }
        Text(stringResource(R.string.automation_boot_hint),style=MaterialTheme.typography.bodySmall)
        Column(Modifier.fillMaxWidth().selectableGroup()){
            Text(stringResource(R.string.automation_after_exit),style=MaterialTheme.typography.labelLarge)
            AutomaticRestartPolicy.entries.forEach{policy->
                Row(Modifier.fillMaxWidth().heightIn(min=48.dp).selectable(
                    selected=restartPolicy==policy,enabled=enabled,role=Role.RadioButton,
                    onClick={policyChanged(policy)}),verticalAlignment=Alignment.CenterVertically){
                    RadioButton(selected=restartPolicy==policy,onClick=null,enabled=enabled)
                    Text(stringResource(when(policy){
                        AutomaticRestartPolicy.Never->R.string.automation_never
                        AutomaticRestartPolicy.OnFailure->R.string.automation_on_failure
                        AutomaticRestartPolicy.Always->R.string.automation_always
                    }),Modifier.padding(start=8.dp))
                }
            }
        }
        Text(stringResource(R.string.automation_manual_stop_hint),style=MaterialTheme.typography.bodySmall)
        Text(stringResource(R.string.automation_save_hint),style=MaterialTheme.typography.bodySmall)
    }
}
