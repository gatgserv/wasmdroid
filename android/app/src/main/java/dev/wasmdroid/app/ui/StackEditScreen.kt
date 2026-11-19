package dev.wasmdroid.app.ui

import androidx.activity.compose.BackHandler
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.runtime.saveable.LocalSaveableStateRegistry
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.ui.unit.dp
import dev.wasmdroid.app.R
import org.json.JSONArray
import org.json.JSONObject

@Composable internal fun StackEditScreen(draft:StackEditState,app:AppState,model:AppModel){
    val controller=model.stackEdit
    val epoch=draft.epoch
    var confirm by remember(draft.id){mutableStateOf<String?>(null)}
    BackHandler{controller.leave(epoch)}
    val members=app.instances.map(::JSONObject).filter{app.memberships[it.getString("id")]==draft.id}
    val count=app.groups.firstOrNull{JSONObject(it).getString("id")==draft.id}?.let{JSONObject(it).getInt("service_count")}
    val active=members.any{it.getString("state") !in listOf("created","exited","dead")||app.controllers[it.getString("id")]!=null}
    val stopped=count!=null&&count==members.size&&!active
    val working=draft.id?.let{id->app.busy.any{it.startsWith("stack:")&&it.endsWith(":$id")}}==true
    LazyColumn(Modifier.fillMaxSize(),contentPadding=PaddingValues(20.dp),verticalArrangement=Arrangement.spacedBy(16.dp)){
        item{TextButton(onClick={controller.leave(epoch)}){Text(stringResource(R.string.back))}}
        item{Text(stringResource(R.string.group_edit_title,draft.id.orEmpty()),style=MaterialTheme.typography.headlineMedium)}
        item{Text(stringResource(R.string.group_edit_preserves))}
        if(draft.applied){
            item{Text(stringResource(R.string.group_edit_applied))}
        }else{
            if(!stopped)item{
                Text(stringResource(R.string.group_edit_stop_first))
                OutlinedButton(enabled=app.connected&&!working&&active,onClick={draft.id?.let(model::stopGroup)}){Text(stringResource(R.string.group_stop))}
            }
            if(draft.version!=null)item{Text(stringResource(R.string.group_edit_revision,draft.version.toString()),style=MaterialTheme.typography.bodySmall)}
            item{CompositionLocalProvider(LocalSaveableStateRegistry provides null){
                OutlinedTextField(value=draft.document,onValueChange={controller.edit(epoch,it)},enabled=app.connected&&!draft.loading&&!draft.applying,
                    label={Text(stringResource(R.string.group_document))},minLines=8,maxLines=16,
                    keyboardOptions=KeyboardOptions(autoCorrectEnabled=false),
                    textStyle=MaterialTheme.typography.bodySmall.copy(fontFamily=FontFamily.Monospace),modifier=Modifier.fillMaxWidth())
            }}
            if(draft.busy)item{LinearProgressIndicator(Modifier.fillMaxWidth())}
            draft.problem?.let{problem->item{Text(stringResource(when(problem){
                StackEditProblem.Unavailable->R.string.group_edit_unavailable
                StackEditProblem.Changed->R.string.group_edit_changed
                StackEditProblem.Busy->R.string.group_edit_busy
                StackEditProblem.Unsupported->R.string.group_edit_unsupported
                StackEditProblem.Invalid->R.string.group_edit_invalid
                StackEditProblem.Large->R.string.group_document_large
                StackEditProblem.Failed->R.string.group_edit_failed
            }),color=MaterialTheme.colorScheme.error)}}
            item{Button(enabled=app.connected&&!draft.busy&&draft.version!=null&&draft.document.isNotBlank(),onClick={controller.review(epoch,draft.revision)}){Text(stringResource(R.string.group_edit_review))}}
            draft.requirements?.let{requirements->item{StackPackageRequirements(requirements,app,model)}}
            draft.preview?.let{raw->
                val preview=JSONObject(raw)
                item{Text(stringResource(R.string.group_edit_changes),style=MaterialTheme.typography.titleMedium)}
                val changes=preview.getJSONObject("changes")
                changes.keys().asSequence().toList().forEach{name->item{
                    val action=when(changes.getString(name)){"keep"->R.string.group_edit_keep;"configure"->R.string.group_edit_configure;"replace"->R.string.group_edit_replace;else->R.string.group_edit_add}
                    Text(stringResource(R.string.group_edit_change,name,stringResource(action)))
                }}
                val removed=preview.getJSONArray("removed")
                for(index in 0 until removed.length())item{Text(stringResource(R.string.group_edit_change,removed.getString(index),stringResource(R.string.group_edit_remove)))}
                item{Text(stringResource(R.string.group_edit_retained,preview.getLong("preserved_volume_count")))}
                item{StackPreview(preview.getJSONObject("plan"))}
                item{Button(enabled=app.connected&&!draft.busy&&!working&&stopped,onClick={controller.apply(epoch,raw)}){Text(stringResource(R.string.group_edit_apply))}}
            }
            item{OutlinedButton(enabled=!draft.busy,onClick={if(draft.dirty)confirm="reload" else controller.reload(epoch)}){Text(stringResource(R.string.group_edit_reload))}}
            item{TextButton(enabled=!draft.applying,onClick={if(draft.dirty)confirm="discard" else controller.discard(epoch)}){Text(stringResource(R.string.group_edit_discard))}}
        }
    }
    if(confirm!=null||draft.pendingId!=null)AlertDialog(
        onDismissRequest={confirm=null;controller.cancelSwitch(epoch)},
        title={Text(stringResource(R.string.group_edit_discard_title))},text={Text(stringResource(R.string.group_edit_discard_body))},
        confirmButton={TextButton(onClick={
            if(draft.pendingId!=null)controller.acceptSwitch(epoch,draft.pendingId) else if(confirm=="reload")controller.reload(epoch) else controller.discard(epoch)
            confirm=null
        }){Text(stringResource(R.string.confirm))}},
        dismissButton={TextButton(onClick={confirm=null;controller.cancelSwitch(epoch)}){Text(stringResource(R.string.cancel))}})
}

@Composable internal fun StackPackageRequirements(raw:String,state:AppState,model:AppModel){
    val requirements=remember(raw){JSONArray(raw)}
    val missing=(0 until requirements.length()).map{requirements.getJSONObject(it)}.filter{!it.getBoolean("installed")}
    if(missing.isEmpty())return
    Column(verticalArrangement=Arrangement.spacedBy(8.dp)){
        Text(stringResource(R.string.group_packages_missing),style=MaterialTheme.typography.titleMedium)
        Text(stringResource(R.string.group_packages_review_again))
        missing.forEach{item->
            val reference=item.getString("reference")
            Text(reference,style=MaterialTheme.typography.bodySmall)
            if(item.getBoolean("downloadable"))OutlinedButton(
                enabled=state.connected&&state.runtimeAvailable&&!state.packageWorkActive()&&!state.packagesUnavailable,
                onClick={model.pull(reference)},
            ){Text(stringResource(R.string.group_package_download))}
            else Text(stringResource(R.string.group_package_import_required),style=MaterialTheme.typography.bodySmall)
        }
    }
}
