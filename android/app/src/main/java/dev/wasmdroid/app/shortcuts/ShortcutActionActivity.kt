package dev.wasmdroid.app.shortcuts

import android.app.Application
import android.app.KeyguardManager
import android.content.Context
import android.content.Intent
import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.compose.foundation.layout.*
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.unit.dp
import androidx.lifecycle.AndroidViewModel
import androidx.lifecycle.ViewModelProvider
import dev.wasmdroid.app.MainActivity
import dev.wasmdroid.app.R
import dev.wasmdroid.app.engine.EngineService
import dev.wasmdroid.app.englishContext
import kotlinx.coroutines.*
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.asStateFlow

internal data class ShortcutActionState(val busy:Boolean=false,val message:Int=R.string.shortcuts_working)

internal class ShortcutActionModel(application:Application):AndroidViewModel(application){
    private val scope=CoroutineScope(SupervisorJob()+Dispatchers.Main.immediate)
    private val mutable=MutableStateFlow(ShortcutActionState())
    val state=mutable.asStateFlow()
    private var connection:ShortcutOwnerConnection?=null
    var begun=false
        private set
    @Volatile private var visible=false
    private var open:((Intent)->Unit)?=null

    fun resumed(launch:(Intent)->Unit){visible=true;open=launch}
    fun paused(){visible=false;open=null}
    private fun unlocked():Boolean{
        val keyguard=getApplication<Application>().getSystemService(KeyguardManager::class.java)
        return keyguard!=null&&!keyguard.isDeviceLocked&&!keyguard.isKeyguardLocked
    }
    private fun gate(click:ShortcutClick)=ShortcutAdmission(click,
        {ShortcutIntents.selectionCurrent(getApplication(),click.selection)},{visible},{unlocked()})
    fun refuse(){if(!begun){begun=true;mutable.value=ShortcutActionState(message=R.string.shortcuts_action_expired)}}

    fun begin(click:ShortcutClick){
        if(begun)return
        begun=true
        if(!unlocked()){mutable.value=ShortcutActionState(message=R.string.shortcuts_unlock_required);return}
        val admission=gate(click)
        if(!admission.current()){mutable.value=ShortcutActionState(message=R.string.shortcuts_selection_changed);return}
        mutable.value=ShortcutActionState(true)
        scope.launch{
            try{
                val context=getApplication<Application>()
                if(click.action==ShortcutAction.Start){
                    // The OS call occurs only from the visible user action.
                    // Any background/FGS denial fails before owner admission.
                    context.startForegroundService(Intent(context,EngineService::class.java))
                }
                connection=ShortcutOwnerConnection(context)
                val owner=requireNotNull(connection).ready.shortcutAwait()
                check(admission.current())
                val target=requireNotNull(click.selection.target)
                val descriptor=ShortcutDescriptor.parse(owner.shortcutResolve(target.json()).shortcutAwait())
                check(admission.resolves(descriptor))
                check(admission.current())
                if(click.action==ShortcutAction.Open||click.action==ShortcutAction.Review){
                    val intent=Intent(context,MainActivity::class.java)
                        .putExtra(ShortcutIntents.EXTRA_TARGET,target.json())
                        .putExtra(ShortcutIntents.EXTRA_STATE_TOKEN,click.stateToken)
                        .putExtra(ShortcutIntents.EXTRA_CLICK,click.json())
                        .putExtra(ShortcutIntents.EXTRA_REVIEW,click.action==ShortcutAction.Review)
                    requireNotNull(open).invoke(intent)
                    mutable.value=ShortcutActionState(message=R.string.shortcuts_opened)
                }else{
                    owner.shortcutAction(target.json(),click.action.wire,click.stateToken,{admission.current()}).shortcutAwait()
                    mutable.value=ShortcutActionState(message=if(click.action==ShortcutAction.Start)R.string.shortcuts_start_completed else R.string.shortcuts_stop_completed)
                }
            }catch(failure:Exception){
                if(failure is CancellationException)throw failure
                mutable.value=ShortcutActionState(message=if(!unlocked())R.string.shortcuts_unlock_required else R.string.shortcuts_action_unavailable)
            }finally{
                connection?.close();connection=null
                ShortcutWidgets.refreshAll(getApplication())
            }
        }
    }
    override fun onCleared() {
        paused()
        scope.cancel()
        connection?.close()
    }
}

/** Widget/tile controls use a visible, private activity rather than guessing a
 * background-FGS exemption. A rotated activity observes one retained request. */
class ShortcutActionActivity:ComponentActivity(){
    private lateinit var model:ShortcutActionModel
    private var click:ShortcutClick?=null
    override fun attachBaseContext(base:Context){super.attachBaseContext(englishContext(base))}
    override fun onCreate(savedInstanceState:Bundle?){
        super.onCreate(savedInstanceState)
        model=ViewModelProvider(this,ViewModelProvider.AndroidViewModelFactory.getInstance(application))[ShortcutActionModel::class.java]
        click=runCatching{ShortcutClick.parse(requireNotNull(intent.getStringExtra(ShortcutIntents.EXTRA_CLICK)))}.getOrNull()
        // A process-restored activity cannot replay a historical user click.
        if(click==null||(savedInstanceState!=null&&!model.begun))model.refuse()
        setContent{
            val state by model.state.collectAsState()
            MaterialTheme{Surface(Modifier.fillMaxSize()){
                Column(Modifier.fillMaxSize().padding(24.dp),verticalArrangement=Arrangement.spacedBy(16.dp)){
                    Text(stringResource(R.string.shortcuts_app_name),style=MaterialTheme.typography.headlineSmall)
                    Text(stringResource(state.message))
                    if(state.busy)CircularProgressIndicator()
                    TextButton(onClick={finish()}){Text(stringResource(R.string.shortcuts_close))}
                }
            }}
        }
    }
    override fun onResume(){
        super.onResume()
        model.resumed{target->startActivity(target);finish()}
        click?.let{model.begin(it)}
    }
    override fun onPause(){model.paused();super.onPause()}
}
