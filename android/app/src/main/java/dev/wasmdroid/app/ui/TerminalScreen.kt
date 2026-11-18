package dev.wasmdroid.app.ui

import android.content.ClipData
import android.content.ClipboardManager
import android.content.Context
import android.os.PersistableBundle
import android.view.KeyEvent
import androidx.activity.compose.BackHandler
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.rememberScrollState
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.unit.dp
import androidx.compose.ui.viewinterop.AndroidView
import dev.wasmdroid.app.R

// The stable ClipDescription extra key is safe on API levels below 33.
private const val SENSITIVE_CLIP_EXTRA = "android.content.extra.IS_SENSITIVE"

@OptIn(ExperimentalMaterial3Api::class)
@Composable internal fun TerminalScreen(id:String,name:String,model:AppModel,back:()->Unit){
    val scope=rememberCoroutineScope()
    var generation by rememberSaveable(id){mutableStateOf<String?>(null)}
    val controller=remember(id){TerminalController(model,id,scope,generation){generation=it}}
    val state by controller.state.collectAsState()
    var view by remember{mutableStateOf<TerminalView?>(null)}
    var control by remember{mutableStateOf(false)}
    var alt by remember{mutableStateOf(false)}
    var clipboardError by remember{mutableStateOf(false)}
    val context=LocalContext.current
    val clipboard=remember(context){context.getSystemService(Context.CLIPBOARD_SERVICE) as ClipboardManager}
    DisposableEffect(controller){onDispose{view=null;controller.close()}}
    BackHandler(onBack=back)
    Scaffold(topBar={TopAppBar(title={Text(stringResource(R.string.terminal_title,name))},navigationIcon={TextButton(onClick=back){Text(stringResource(R.string.back))}})}){padding->
        Column(Modifier.fillMaxSize().padding(padding).imePadding(),verticalArrangement=Arrangement.spacedBy(4.dp)){
            Text(stringResource(if(state.live)R.string.terminal_live else if(state.generation==null)R.string.connecting else R.string.terminal_ended),modifier=Modifier.padding(horizontal=16.dp),style=MaterialTheme.typography.labelMedium)
            Row(Modifier.horizontalScroll(rememberScrollState()).padding(horizontal=8.dp)){
                TextButton(enabled=state.live&&state.inputOpen,onClick={view?.showKeyboard()}){Text(stringResource(R.string.terminal_keyboard))}
                TextButton(enabled=state.frame!=null,onClick={
                    val text=state.frame?.plainText()?:""
                    if(text.toByteArray(Charsets.UTF_8).size>64*1024)clipboardError=true
                    else clipboard.setPrimaryClip(ClipData.newPlainText("Terminal",text).apply{description.extras=PersistableBundle().apply{putBoolean(SENSITIVE_CLIP_EXTRA,true)}})
                }){Text(stringResource(R.string.terminal_copy))}
                TextButton(enabled=state.live&&state.inputOpen,onClick={
                    val text=clipboard.primaryClip?.takeIf{it.itemCount>0}?.getItemAt(0)?.text?.toString()
                    if(text==null)clipboardError=true else controller.paste(text)
                }){Text(stringResource(R.string.terminal_paste))}
                TextButton(enabled=state.live,onClick=controller::interrupt){Text(stringResource(R.string.terminal_interrupt))}
                TextButton(enabled=state.live&&state.inputOpen,onClick=controller::eof){Text(stringResource(R.string.terminal_eof))}
                TextButton(enabled=state.live,onClick={model.stop(id)}){Text(stringResource(R.string.stop))}
            }
            state.error?.let{ErrorCard(it,true,controller::dismissError)}
            if(clipboardError)ErrorCard(stringResource(R.string.terminal_clipboard_error),true){clipboardError=false}
            if(state.frame?.replyOverflow==true)Text(stringResource(R.string.terminal_reply_pressure),Modifier.padding(horizontal=16.dp),style=MaterialTheme.typography.bodySmall)
            if(state.frame==null&&state.generation==null)LinearProgressIndicator(Modifier.fillMaxWidth())
            if((state.frame?.scrollback?:0)>0){TextButton(onClick=controller::liveView){Text(stringResource(R.string.terminal_latest))}}
            AndroidView(modifier=Modifier.weight(1f).fillMaxWidth(),factory={ctx->TerminalView(ctx).apply{
                view=this;onInput=controller::send;onResize=controller::resize;onScroll=controller::scrollBy;onModifiersConsumed={control=false;alt=false};onInputError=controller::inputError
            }},update={terminal->terminal.inputEnabled=state.live&&state.inputOpen;terminal.controlLatched=control;terminal.altLatched=alt;terminal.update(state.frame)})
            Row(Modifier.horizontalScroll(rememberScrollState()).padding(horizontal=8.dp),horizontalArrangement=Arrangement.spacedBy(4.dp)){
                FilterChip(selected=control,onClick={control=!control},enabled=state.live&&state.inputOpen,label={Text("Ctrl")})
                FilterChip(selected=alt,onClick={alt=!alt},enabled=state.live&&state.inputOpen,label={Text("Alt")})
                listOf("Esc" to KeyEvent.KEYCODE_ESCAPE,"Tab" to KeyEvent.KEYCODE_TAB,"←" to KeyEvent.KEYCODE_DPAD_LEFT,"↑" to KeyEvent.KEYCODE_DPAD_UP,"↓" to KeyEvent.KEYCODE_DPAD_DOWN,"→" to KeyEvent.KEYCODE_DPAD_RIGHT,"Enter" to KeyEvent.KEYCODE_ENTER).forEach{(label,key)->
                    TextButton(enabled=state.live&&state.inputOpen,onClick={view?.onKeyDown(key,KeyEvent(KeyEvent.ACTION_DOWN,key))}){Text(label)}
                }
            }
        }
    }
}
