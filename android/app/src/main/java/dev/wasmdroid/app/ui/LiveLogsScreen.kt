package dev.wasmdroid.app.ui

import androidx.activity.compose.BackHandler
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.unit.dp
import dev.wasmdroid.app.R
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.delay
import org.json.JSONObject

@OptIn(ExperimentalMaterial3Api::class)
@Composable internal fun LiveLogsScreen(id:String,name:String,model:AppModel,back:()->Unit){
    val buffer=remember(id){LogBuffer()}
    var filter by rememberSaveable(id){mutableStateOf("both")}
    var follow by rememberSaveable(id){mutableStateOf(true)}
    var refresh by remember(id){mutableIntStateOf(0)}
    var text by remember(id){mutableStateOf("")}
    var missing by remember(id){mutableStateOf(false)}
    var incomplete by remember(id){mutableStateOf(false)}
    var error by remember(id){mutableStateOf<String?>(null)}
    val scroll=rememberScrollState()
    BackHandler(onBack=back)
    LaunchedEffect(id,follow,refresh,filter){
        do {
            var more=false
            try{
                val query=JSONObject().put("after",buffer.cursor)
                if(buffer.cursor==0L)query.put("tail",200)
                val raw=model.logs(id,query.toString())
                if(raw!=null){val page=JSONObject(raw);buffer.accept(page);text=buffer.text(filter);more=page.optBoolean("has_more")}
                else text=model.output(id)?.let{JSONObject(it).let{value->when(filter){"stdout"->value.optString("stdout");"stderr"->value.optString("stderr");else->value.optString("stdout")+value.optString("stderr")}}}?:""
                missing=buffer.historyMissing;incomplete=buffer.incomplete;error=null
            }catch(failure:Exception){if(failure is CancellationException)throw failure;error="Unable to read retained service output"}
            if(follow)delay(if(more)50 else 500)
        }while(follow)
    }
    LaunchedEffect(text,follow){if(follow)scroll.scrollTo(scroll.maxValue)}
    Scaffold(topBar={TopAppBar(title={Text(stringResource(R.string.logs_title,name))},navigationIcon={TextButton(onClick=back){Text(stringResource(R.string.back))}})}){padding->
        Column(Modifier.fillMaxSize().padding(padding).padding(16.dp),verticalArrangement=Arrangement.spacedBy(12.dp)){
            Row(horizontalArrangement=Arrangement.spacedBy(8.dp)){
                listOf("both" to R.string.logs_both,"stdout" to R.string.logs_stdout,"stderr" to R.string.logs_stderr).forEach{(value,label)->
                    FilterChip(selected=filter==value,onClick={filter=value},label={Text(stringResource(label))})
                }
            }
            Row(horizontalArrangement=Arrangement.spacedBy(12.dp)){
                Checkbox(checked=follow,onCheckedChange={follow=it});Text(stringResource(R.string.logs_follow),Modifier.weight(1f).padding(top=12.dp))
                TextButton(onClick={buffer.clear();refresh++}){Text(stringResource(R.string.reload))}
            }
            if(missing)Text(stringResource(R.string.logs_retained),style=MaterialTheme.typography.bodySmall)
            if(incomplete)Text(stringResource(R.string.logs_incomplete),style=MaterialTheme.typography.bodySmall)
            error?.let{ErrorCard(it,true){error=null}}
            Text(text.ifEmpty{stringResource(R.string.logs_empty)},fontFamily=FontFamily.Monospace,
                modifier=Modifier.weight(1f).fillMaxWidth().verticalScroll(scroll))
        }
    }
}
