package dev.wasmdroid.app.ui

import android.net.Uri
import android.provider.OpenableColumns
import androidx.activity.compose.BackHandler
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.unit.dp
import dev.wasmdroid.app.R
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import kotlinx.coroutines.CancellationException
import org.json.JSONObject

private data class DocumentSource(val uri:Uri,val name:String,val size:Long?)
@OptIn(ExperimentalMaterial3Api::class)
@Composable fun AppDataScreen(recordJson:String,state:AppState,model:AppModel,back:()->Unit){
    val row=remember(recordJson){JSONObject(recordJson)}
    val id=row.getString("id");val spec=row.getJSONObject("spec")
    val title=spec.optJSONObject("labels")?.optString("dev.wasmdroid.ui.name")?.takeIf{it.isNotBlank()}?:spec.getString("command")
    val volumes=remember(recordJson){
        val array=spec.optJSONArray("mounts")
        (0 until (array?.length()?:0)).mapNotNull{index->array!!.getJSONObject(index).getJSONObject("source").let{source->
            source.optString("source").takeIf{source.optString("type")=="volume"}
        }}.distinct()
    }
    var volume by rememberSaveable(id){mutableStateOf(volumes.firstOrNull())}
    var path by rememberSaveable(id,volume){mutableStateOf("")}
    var entries by remember(id,volume,path){mutableStateOf<List<String>>(emptyList())}
    var loading by remember{mutableStateOf(false)}
    var listingError by remember{mutableStateOf(false)}
    var importUri by rememberSaveable(id,volume){mutableStateOf<Uri?>(null)}
    var source by remember{mutableStateOf<DocumentSource?>(null)}
    var filename by rememberSaveable(id,volume){mutableStateOf("")}
    var nameEdited by rememberSaveable(id,volume){mutableStateOf(false)}
    var folder by rememberSaveable(id,volume){mutableStateOf<String?>(null)}
    var delete by remember{mutableStateOf<String?>(null)}
    var exportPath by rememberSaveable(id,volume){mutableStateOf<String?>(null)}
    var restoreUri by rememberSaveable(id,volume){mutableStateOf<Uri?>(null)}
    val busy="data:$id" in state.busy
    val idle=row.getString("state") !in listOf("running","paused","restarting")&&state.controllers[id]==null
    val editable=idle&&!busy
    val context=LocalContext.current
    val importPicker=rememberLauncherForActivityResult(ActivityResultContracts.OpenDocument()){importUri=it}
    val exportPicker=rememberLauncherForActivityResult(ActivityResultContracts.CreateDocument("application/octet-stream")){uri->
        val selected=exportPath;val current=volume
        if(uri!=null&&selected!=null&&current!=null)model.exportFile(id,current,selected,uri)
        exportPath=null
    }
    val backupPicker=rememberLauncherForActivityResult(ActivityResultContracts.CreateDocument("application/gzip")){uri->
        volume?.let{if(uri!=null)model.backup(id,it,uri)}
    }
    val restorePicker=rememberLauncherForActivityResult(ActivityResultContracts.OpenDocument()){restoreUri=it}
    LaunchedEffect(volumes){if(volume !in volumes){volume=volumes.firstOrNull();path=""}}
    LaunchedEffect(volume,path,state.dataRevision,busy){
        if(volume!=null&&!busy){
            loading=true;listingError=false
            try{entries=model.files(volume!!,path)}catch(failure:Exception){if(failure is CancellationException)throw failure;listingError=true}
            finally{loading=false}
        }
    }
    LaunchedEffect(importUri){
        val uri=importUri?:return@LaunchedEffect
        try{
            source=withContext(Dispatchers.IO){
                var name="document";var size:Long?=null
                context.contentResolver.query(uri,arrayOf(OpenableColumns.DISPLAY_NAME,OpenableColumns.SIZE),null,null,null)?.use{cursor->
                    if(cursor.moveToFirst()){
                        val nameColumn=cursor.getColumnIndex(OpenableColumns.DISPLAY_NAME)
                        val sizeColumn=cursor.getColumnIndex(OpenableColumns.SIZE)
                        if(nameColumn>=0&&!cursor.isNull(nameColumn))name=cursor.getString(nameColumn)
                        if(sizeColumn>=0&&!cursor.isNull(sizeColumn))size=cursor.getLong(sizeColumn).takeIf{it>=0}
                    }
                }
                DocumentSource(uri,name,size)
            }
            if(!nameEdited)filename=source?.name?:"document"
        }catch(failure:Exception){if(failure is CancellationException)throw failure;listingError=true;importUri=null}
    }
    BackHandler(onBack=back)
    Scaffold(topBar={TopAppBar(title={Text(stringResource(R.string.app_data,title))},navigationIcon={TextButton(onClick=back){Text(stringResource(R.string.back))}})}){padding->
        Column(Modifier.fillMaxSize().padding(padding)){
            state.error?.let{ErrorCard(it,state.advanced,dev.wasmdroid.app.reports.ReportIssue.fromCodes(state.errorKind,state.errorOperation,state.errorStage),model::dismissError)}
            state.notice?.let{NoticeCard(it,model::dismissNotice)}
            if(busy){OperationProgress(state.progress);TextButton(onClick={model.cancelData(id)},modifier=Modifier.padding(horizontal=20.dp)){Text(stringResource(R.string.cancel))}}
            LazyColumn(Modifier.fillMaxSize(),contentPadding=PaddingValues(20.dp),verticalArrangement=Arrangement.spacedBy(14.dp)){
                item{BrowserDataPane(id,title,state.connected,model)}
                if(volumes.isEmpty())item{Text(stringResource(R.string.no_app_data))}
                if(volumes.size>1)item{Column(verticalArrangement=Arrangement.spacedBy(8.dp)){
                    volumes.forEachIndexed{index,name->FilterChip(selected=volume==name,onClick={if(!busy){volume=name;path=""}},label={Text(if(state.advanced)name else stringResource(R.string.data_set,index+1))})}
                }}
                if(volume!=null){
                    if(!idle&&!busy)item{OutlinedCard{Column(Modifier.padding(16.dp),verticalArrangement=Arrangement.spacedBy(10.dp)){
                        Text(stringResource(R.string.data_stop_first))
                        if(row.getString("state")=="running")OutlinedButton(enabled=id !in state.busy,onClick={model.stop(id)}){Text(stringResource(R.string.stop))}
                    }}}
                    item{Row(horizontalArrangement=Arrangement.spacedBy(8.dp)){
                        OutlinedButton(enabled=editable,onClick={nameEdited=false;importPicker.launch(arrayOf("*/*"))}){Text(stringResource(R.string.import_file))}
                        OutlinedButton(enabled=editable,onClick={folder=""}){Text(stringResource(R.string.new_folder))}
                    }}
                    item{Text(stringResource(R.string.files),style=MaterialTheme.typography.titleLarge)}
                    if(path.isNotEmpty())item{TextButton(enabled=!busy,onClick={path=path.substringBeforeLast('/',"")}){Text(stringResource(R.string.parent_folder))};Text(path,style=MaterialTheme.typography.bodySmall)}
                    if(loading)item{LinearProgressIndicator(Modifier.fillMaxWidth())}
                    if(listingError)item{Text(stringResource(R.string.data_list_error),color=MaterialTheme.colorScheme.error)}
                    if(!loading&&!listingError&&entries.isEmpty())item{Text(stringResource(R.string.empty_folder))}
                    items(entries,key={JSONObject(it).getString("name")}){json->
                        val entry=JSONObject(json);val name=entry.getString("name")
                        val directory=entry.getBoolean("is_dir");val link=entry.getBoolean("is_symlink")
                        val target=if(path.isEmpty())name else "$path/$name"
                        OutlinedCard(Modifier.fillMaxWidth()){Column(Modifier.padding(16.dp),verticalArrangement=Arrangement.spacedBy(8.dp)){
                            Text(name,style=MaterialTheme.typography.titleMedium)
                            if(directory&&!link)TextButton(enabled=!busy,onClick={path=target}){Text(stringResource(R.string.open_folder))}
                            else if(entry.getBoolean("is_file")&&!link){
                                Text(stringResource(R.string.file_size_kib,(entry.getLong("size_bytes")+1023)/1024),style=MaterialTheme.typography.bodySmall)
                                TextButton(enabled=editable,onClick={exportPath=target;exportPicker.launch(name)}){Text(stringResource(R.string.export_file))}
                            }
                            TextButton(enabled=editable,onClick={delete=json}){Text(stringResource(R.string.delete_file))}
                        }}
                    }
                    item{HorizontalDivider();Text(stringResource(R.string.backup),style=MaterialTheme.typography.titleLarge,modifier=Modifier.padding(top=14.dp));Text(stringResource(R.string.backup_body))}
                    item{Column(verticalArrangement=Arrangement.spacedBy(8.dp)){
                        Button(enabled=editable,onClick={backupPicker.launch("WasmDroid-data-${java.time.LocalDate.now()}.tar.gz")}){Text(stringResource(R.string.save_backup))}
                        OutlinedButton(enabled=editable,onClick={restorePicker.launch(arrayOf("*/*"))}){Text(stringResource(R.string.restore_backup))}
                    }}
                }
            }
        }
    }
    source?.let{selected->
        val exists=entries.any{JSONObject(it).getString("name")==filename}
        val valid=filename.isNotBlank()&&filename !in listOf(".","..")&&!filename.contains('/')&&!filename.contains('\\')&&!filename.any{it.isISOControl()}
        AlertDialog(onDismissRequest={source=null;importUri=null},title={Text(stringResource(if(exists)R.string.replace_file else R.string.import_file))},
            text={Column(verticalArrangement=Arrangement.spacedBy(12.dp)){
                OutlinedTextField(value=filename,onValueChange={filename=it;nameEdited=true},label={Text(stringResource(R.string.filename))},singleLine=true)
                if(exists)Text(stringResource(R.string.replace_file_warning))
            }},confirmButton={Button(enabled=valid&&editable,onClick={
                volume?.let{model.importFile(id,it,if(path.isEmpty())filename else "$path/$filename",selected.uri,exists,selected.size)}
                source=null;importUri=null
            }){Text(stringResource(if(exists)R.string.confirm_replace else R.string.import_file))}},dismissButton={TextButton(onClick={source=null;importUri=null}){Text(stringResource(R.string.cancel))}})
    }
    folder?.let{value->
        val valid=value.isNotBlank()&&value !in listOf(".","..")&&!value.contains('/')&&!value.contains('\\')&&!value.any{it.isISOControl()}
        AlertDialog(onDismissRequest={folder=null},title={Text(stringResource(R.string.new_folder))},text={OutlinedTextField(value=value,onValueChange={folder=it},singleLine=true,label={Text(stringResource(R.string.folder_name))})},
            confirmButton={Button(enabled=valid&&editable,onClick={volume?.let{model.createFolder(id,it,if(path.isEmpty())value else "$path/$value")};folder=null}){Text(stringResource(R.string.create_folder))}},dismissButton={TextButton(onClick={folder=null}){Text(stringResource(R.string.cancel))}})
    }
    delete?.let{json->
        val entry=JSONObject(json);val name=entry.getString("name")
        AlertDialog(onDismissRequest={delete=null},title={Text(stringResource(R.string.delete_file_title,name))},text={Text(stringResource(R.string.delete_file_warning))},
            confirmButton={TextButton(enabled=editable,onClick={volume?.let{model.deleteFile(id,it,if(path.isEmpty())name else "$path/$name",entry.getBoolean("is_dir")&&!entry.getBoolean("is_symlink"))};delete=null}){Text(stringResource(R.string.delete_file))}},dismissButton={TextButton(onClick={delete=null}){Text(stringResource(R.string.cancel))}})
    }
    restoreUri?.let{uri->AlertDialog(onDismissRequest={restoreUri=null},title={Text(stringResource(R.string.restore_backup))},text={Text(stringResource(R.string.restore_preserves_data))},
        confirmButton={Button(enabled=editable,onClick={volume?.let{model.restore(id,it,uri)};restoreUri=null}){Text(stringResource(R.string.restore_backup))}},dismissButton={TextButton(onClick={restoreUri=null}){Text(stringResource(R.string.cancel))}})}
}
