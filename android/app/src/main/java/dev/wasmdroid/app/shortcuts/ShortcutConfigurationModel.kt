package dev.wasmdroid.app.shortcuts

import android.app.Application
import androidx.lifecycle.AndroidViewModel
import dev.wasmdroid.app.R
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.cancel
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.launch
import kotlinx.coroutines.suspendCancellableCoroutine
import kotlinx.coroutines.withContext
import org.json.JSONObject
import java.util.concurrent.CompletableFuture

internal suspend fun <T> CompletableFuture<T>.shortcutAwait():T=suspendCancellableCoroutine{continuation->
    // Cancellation of a UI observer never completes/cancels an owner future.
    whenComplete{value,failure->if(continuation.isActive){
        if(failure==null)continuation.resumeWith(Result.success(value))
        else continuation.resumeWith(Result.failure(failure))
    }}
}

internal data class ShortcutConfigurationState(
    val selection:ShortcutSelection?=null,val targets:List<ShortcutDescriptor> = emptyList(),
    val chosen:ShortcutTarget?=null,val next:String?=null,val busy:Boolean=true,
    val problem:Int?=null,val saved:Boolean=false,val epoch:Long=0,
)

internal class ShortcutConfigurationModel(application:Application):AndroidViewModel(application){
    private val scope=CoroutineScope(SupervisorJob()+Dispatchers.Main.immediate)
    private val mutable=MutableStateFlow(ShortcutConfigurationState())
    val state=mutable.asStateFlow()
    private var connection:ShortcutOwnerConnection?=null
    private var initialized=false
    private var closed=false
    private var surface:String?=null

    fun initialize(surface:String,expected:ShortcutSelection?){
        if(initialized)return
        initialized=true
        this.surface=surface
        scope.launch{
            try{
                val selection=withContext(Dispatchers.IO){ShortcutSelections.current(getApplication(),surface)}
                check(expected==null||selection==expected)
                mutable.value=ShortcutConfigurationState(selection=selection,chosen=selection.target,epoch=1)
                connection=ShortcutOwnerConnection(getApplication())
                loadPage(1,null)
            }catch(failure:Exception){
                if(failure is CancellationException)throw failure
                if(!closed)mutable.value=mutable.value.copy(busy=false,problem=R.string.shortcuts_selection_changed)
            }
        }
    }
    fun choose(epoch:Long,target:ShortcutTarget){
        val old=mutable.value
        if(old.epoch!=epoch||old.busy||old.saved||old.targets.none{it.target==target})return
        mutable.value=old.copy(chosen=target,problem=null)
    }
    fun more(epoch:Long){
        val old=mutable.value
        if(old.epoch!=epoch||old.busy||old.saved)return
        old.next?.let{cursor->scope.launch{loadPage(epoch,cursor)}}
    }
    private suspend fun loadPage(epoch:Long,cursor:String?){
        val old=mutable.value
        if(old.epoch!=epoch||closed)return
        mutable.value=old.copy(busy=true,problem=null)
        try{
            val owner=requireNotNull(connection).ready.shortcutAwait()
            val json=owner.shortcutTargets(cursor,50).shortcutAwait()
            check(json.length<=256*1024)
            val page=JSONObject(json)
            val entries=page.getJSONArray("targets")
            check(entries.length()<=50)
            val parsed=(0 until entries.length()).map{ShortcutDescriptor.parse(entries.getJSONObject(it).toString())}
            if(mutable.value.epoch==epoch&&!closed){
                val current=mutable.value
                // Retain at most a technical page window; Next explicitly moves
                // to the next page instead of silently hiding later apps.
                mutable.value=current.copy(targets=parsed,
                    chosen=parsed.firstOrNull{current.chosen?.sameIdentity(it.target)==true}?.target?:current.chosen,
                    next=page.optString("next").takeIf{it.isNotEmpty()&&it!="null"},busy=false)
            }
        }catch(failure:Exception){
            if(failure is CancellationException)throw failure
            if(mutable.value.epoch==epoch&&!closed)mutable.value=mutable.value.copy(busy=false,problem=R.string.shortcuts_unavailable)
        }
    }
    fun reload(epoch:Long){
        val old=mutable.value
        if(old.epoch!=epoch||old.busy||old.saved)return
        val nextEpoch=old.epoch+1
        mutable.value=old.copy(epoch=nextEpoch,busy=true,problem=null)
        scope.launch{
            try{
                val selection=withContext(Dispatchers.IO){ShortcutSelections.current(getApplication(),requireNotNull(surface))}
                if(closed||mutable.value.epoch!=nextEpoch)return@launch
                connection?.close();connection=ShortcutOwnerConnection(getApplication())
                mutable.value=mutable.value.copy(selection=selection,chosen=selection.target,targets=emptyList(),next=null)
                loadPage(nextEpoch,null)
            }catch(failure:Exception){
                if(failure is CancellationException)throw failure
                if(!closed&&mutable.value.epoch==nextEpoch)mutable.value=mutable.value.copy(busy=false,problem=R.string.shortcuts_unavailable)
            }
        }
    }
    fun save(epoch:Long,target:ShortcutTarget){
        val old=mutable.value
        if(old.epoch!=epoch||old.busy||old.saved||old.chosen!=target||old.targets.none{it.target==target})return
        val expected=old.selection?:return
        mutable.value=old.copy(busy=true,problem=null)
        scope.launch{
            try{
                val owner=requireNotNull(connection).ready.shortcutAwait()
                val actual=ShortcutDescriptor.parse(owner.shortcutResolve(target.json()).shortcutAwait())
                check(actual.target.sameIdentity(target))
                val saved=withContext(Dispatchers.IO){ShortcutSelections.replace(getApplication(),expected,actual.target)}
                if(!closed)mutable.value=mutable.value.copy(selection=saved,busy=false,saved=true)
                ShortcutWidgets.refreshAll(getApplication())
            }catch(failure:Exception){
                if(failure is CancellationException)throw failure
                if(!closed)mutable.value=mutable.value.copy(busy=false,problem=R.string.shortcuts_selection_changed)
            }
        }
    }
    override fun onCleared() {
        closed = true
        scope.cancel()
        connection?.close()
    }
}
