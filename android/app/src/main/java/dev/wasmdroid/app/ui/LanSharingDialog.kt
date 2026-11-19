package dev.wasmdroid.app.ui

import android.content.ClipData
import android.content.ClipboardManager
import androidx.compose.foundation.Image
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.text.selection.SelectionContainer
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.ImageBitmap
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.unit.dp
import androidx.core.graphics.createBitmap
import dev.wasmdroid.app.R
import kotlinx.coroutines.*
import org.json.JSONObject

@OptIn(ExperimentalLayoutApi::class)
@Composable internal fun LanSharingDialog(target:String,model:AppModel,localNetwork:LocalNetworkAccess,dismiss:()->Unit) {
    val context=LocalContext.current
    val scope=rememberCoroutineScope()
    var frame by remember(target){mutableStateOf<JSONObject?>(null)}
    var statusAvailable by remember(target){mutableStateOf(false)}
    var selected by rememberSaveable(target){mutableStateOf<String?>(null)}
    var starting by remember(target){mutableStateOf(false)}
    var startIntent by remember(target){mutableStateOf<dev.wasmdroid.app.engine.LanStartIntent?>(null)}
    var stopping by remember(target){mutableStateOf(false)}
    var userStopped by remember(target){mutableStateOf(false)}
    var locallyStoppedEpoch by remember(target){mutableStateOf<Long?>(null)}
    var actionError by remember(target){mutableStateOf<Int?>(null)}
    var copied by remember(target){mutableStateOf(false)}
    val displayFence=remember(target){java.util.concurrent.atomic.AtomicReference(Any())}
    val mounted=remember(target){java.util.concurrent.atomic.AtomicBoolean(true)}
    fun invalidateDisplay(){displayFence.set(Any());copied=false}
    fun dismissSafely(){mounted.set(false);invalidateDisplay();dismiss()}
    DisposableEffect(target){onDispose{mounted.set(false);displayFence.set(Any())}}
    LaunchedEffect(target,localNetwork.granted){
        if(!localNetwork.granted){
            startIntent?.cancel()
            invalidateDisplay()
        }
    }
    suspend fun refresh(){
        val next=JSONObject(model.sharingStatus(target))
        if(!statusAvailable||frame?.optString("phase")!=next.optString("phase")||
            frame?.optLong("grant_epoch")!=next.optLong("grant_epoch")||frame?.optString("url")!=next.optString("url")||
            frame?.optString("ipv6_url")!=next.optString("ipv6_url"))invalidateDisplay()
        frame=next;statusAvailable=true
    }
    LaunchedEffect(target){
        while(isActive){
            try{refresh()}catch(failure:Exception){if(failure is CancellationException)throw failure;statusAvailable=false;invalidateDisplay()}
            delay(500)
        }
    }
    val phase=frame?.optString("phase")?:"off"
    val epoch=frame?.optLong("grant_epoch",0)?:0
    val choices=frame?.optJSONArray("candidates")
    val candidates=(0 until (choices?.length()?:0)).map{choices!!.getJSONObject(it)}
    val tokens=candidates.map{it.getString("selection_token")}
    LaunchedEffect(tokens){if(selected==null||tokens.none{it==selected})selected=tokens.firstOrNull()}
    val addressesVisible = localNetwork.granted&&statusAvailable&&!stopping&&!(userStopped&&starting)&&
        epoch!=locallyStoppedEpoch&&phase=="active"
    val addresses = frame?.takeIf { addressesVisible }?.let(ActiveLanAddresses::fromFrame)
    val url = addresses?.ipv4
    val ipv6Url = addresses?.ipv6
    fun copyAddress(field: String, address: String) {
        val ticket = displayFence.get()
        scope.launch {
            try {
                val current=JSONObject(model.sharingStatus(target))
                check(current.getString("phase")=="active"&&current.getLong("grant_epoch")==epoch&&current.getString(field)==address)
                check(localNetwork.refresh()&&mounted.get()&&displayFence.get()===ticket&&model.sharingUiCurrent()&&statusAvailable&&!stopping&&locallyStoppedEpoch!=epoch&&
                    frame?.optString("phase")=="active"&&frame?.optLong("grant_epoch")==epoch&&frame?.optString(field)==address)
                val clipboard=checkNotNull(context.getSystemService(ClipboardManager::class.java))
                clipboard.setPrimaryClip(ClipData.newPlainText("WasmDroid Wi-Fi address",address))
                copied=true
            }catch(failure:Exception){if(failure is CancellationException)throw failure;copied=false;actionError=R.string.lan_clipboard_failed}
        }
    }
    val qr by produceState<Pair<String,Result<ImageBitmap>>?>(null,url){
        value=null
        if(url!=null)value=url to withContext(Dispatchers.Default){runCatching{
            val matrix=lanQrMatrix(url)
            val pixels=IntArray(matrix.width*matrix.height){index->
                if(matrix[index%matrix.width,index/matrix.width])android.graphics.Color.BLACK else android.graphics.Color.WHITE
            }
            createBitmap(matrix.width,matrix.height).apply{
                setPixels(pixels,0,matrix.width,0,0,matrix.width,matrix.height)
            }.asImageBitmap()
        }}
    }
    val currentQr=qr?.takeIf{it.first==url}?.second
    AlertDialog(onDismissRequest=::dismissSafely,title={Text(stringResource(R.string.lan_share))},text={
        Column(Modifier.verticalScroll(rememberScrollState()),verticalArrangement=Arrangement.spacedBy(12.dp)){
            Text(stringResource(when{
                !statusAvailable->R.string.lan_status_unavailable
                userStopped&&starting->R.string.lan_stopping
                phase=="stopping"->R.string.lan_stopping
                phase=="uncertain"->R.string.lan_uncertain
                phase=="app_stopped"->R.string.lan_app_stopped
                !localNetwork.granted->R.string.lan_off
                phase=="starting"||starting&&phase=="off"->R.string.lan_starting
                phase=="active"->R.string.lan_active
                phase=="unavailable"->R.string.lan_unavailable
                candidates.isEmpty()->R.string.lan_no_wifi
                else->R.string.lan_off
            }),style=MaterialTheme.typography.titleMedium)
            Text(stringResource(R.string.lan_intro))
            Text(stringResource(R.string.lan_exposure),style=MaterialTheme.typography.bodySmall)
            if(!localNetwork.granted&&statusAvailable&&phase=="off"){
                LocalNetworkAccessActions(localNetwork,R.string.lan_android_permission)
            }
            if(localNetwork.granted&&phase=="off"&&statusAvailable&&candidates.isNotEmpty()){
                if(candidates.size>1)Text(stringResource(R.string.lan_select_address))
                candidates.forEach{candidate->
                    Row(verticalAlignment=Alignment.CenterVertically){
                        if(candidates.size>1)RadioButton(selected=selected==candidate.getString("selection_token"),onClick={selected=candidate.getString("selection_token")})
                        Column {
                            Text(candidate.getString("private_address"))
                            (candidate.opt("ipv6_address") as? String)?.let { Text(it) }
                        }
                    }
                }
            }
            FlowRow(horizontalArrangement=Arrangement.spacedBy(8.dp),verticalArrangement=Arrangement.spacedBy(4.dp)){
                if(localNetwork.granted&&phase=="off")Button(enabled=statusAvailable&&selected!=null&&tokens.any{it==selected}&&!starting&&!stopping,onClick={
                    if(!localNetwork.refresh())return@Button
                    val token=selected?:return@Button
                    val stateToken=frame?.optString("state_token")?.takeIf{it.isNotBlank()}?:return@Button
                    starting=true;userStopped=false;locallyStoppedEpoch=null;actionError=null;invalidateDisplay()
                    val intent=dev.wasmdroid.app.engine.LanStartIntent();startIntent=intent
                    scope.launch{
                        try{model.startSharing(target,stateToken,token,intent);refresh()}
                        catch(failure:Exception){if(failure is CancellationException)throw failure;if(!userStopped)actionError=R.string.lan_action_failed}
                        finally{starting=false}
                    }
                }){Text(stringResource(R.string.lan_start))}
                if(starting||epoch>0&&phase in setOf("starting","active","stopping","uncertain"))OutlinedButton(enabled=!stopping,onClick={
                    startIntent?.cancel()
                    stopping=true;userStopped=true;locallyStoppedEpoch=epoch;actionError=null;invalidateDisplay()
                    scope.launch{
                        try{if(epoch>0)model.stopSharing(target,epoch);refresh()}
                        catch(failure:Exception){if(failure is CancellationException)throw failure;actionError=R.string.lan_action_failed}
                        finally{stopping=false}
                    }
                }){Text(stringResource(R.string.lan_stop))}
            }
            if(starting||stopping||phase in setOf("starting","stopping"))LinearProgressIndicator(Modifier.fillMaxWidth())
            if(url!=null){
                if (ipv6Url != null) Text(stringResource(R.string.lan_ipv4),style=MaterialTheme.typography.labelMedium)
                SelectionContainer{Text(url,style=MaterialTheme.typography.bodySmall)}
                TextButton(onClick={copyAddress("url", url)}){Text(stringResource(R.string.lan_copy))}
            }
            if (ipv6Url != null) {
                Text(stringResource(R.string.lan_ipv6),style=MaterialTheme.typography.labelMedium)
                SelectionContainer{Text(ipv6Url,style=MaterialTheme.typography.bodySmall)}
                TextButton(onClick={copyAddress("ipv6_url", ipv6Url)}){Text(stringResource(R.string.lan_copy_ipv6))}
            }
            currentQr?.getOrNull()?.let { bitmap ->
                Box(Modifier.fillMaxWidth(), contentAlignment = Alignment.Center) {
                    Image(bitmap, contentDescription = stringResource(R.string.lan_qr_description), modifier = Modifier.size(220.dp))
                }
            }
            if(currentQr?.isFailure==true)Text(stringResource(R.string.lan_qr_unavailable),style=MaterialTheme.typography.bodySmall)
            if(copied)Text(stringResource(R.string.lan_copied),style=MaterialTheme.typography.bodySmall)
            if(frame?.optBoolean("failed")==true&&phase=="off")Text(stringResource(R.string.lan_closed_error),color=MaterialTheme.colorScheme.error)
            actionError?.let{Text(stringResource(it),color=MaterialTheme.colorScheme.error)}
            if(actionError!=null)DiagnosticReportButton(dev.wasmdroid.app.reports.ReportIssue.fromCodes("other","share","failed"))
            Text(
                stringResource(if (frame?.optBoolean("default_sharing")==true) R.string.lan_lifetime_default else R.string.lan_lifetime),
                style=MaterialTheme.typography.bodySmall,
            )
        }
    },confirmButton={TextButton(onClick=::dismissSafely){Text(stringResource(R.string.close))}})
}
