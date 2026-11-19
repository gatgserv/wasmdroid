package dev.wasmdroid.app.ui

import kotlinx.coroutines.*
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.update

internal data class NetworkPermissionSource(
    val id:String,val identity:String,val version:ULong,val name:String,val status:String,
    val managed:Boolean,val networkEnabled:Boolean,val internet:Boolean,val lan:Boolean,val supported:Boolean,
) {override fun toString()="NetworkPermissionSource(private)"}
internal interface NetworkPermissionBackend {
    val current:Boolean
    suspend fun source(id:String):NetworkPermissionSource
    suspend fun save(source:NetworkPermissionSource,internet:Boolean,lan:Boolean)
}
internal enum class NetworkPermissionProblem {Unavailable,Changed,Unsupported,Save}
internal data class NetworkPermissionState(
    val open:Boolean=false,val epoch:Long=0,val id:String?=null,val source:NetworkPermissionSource?=null,
    val internet:Boolean=false,val lan:Boolean=false,val loading:Boolean=false,val saving:Boolean=false,
    val problem:NetworkPermissionProblem?=null,
){val busy:Boolean get()=loading||saving;override fun toString()="NetworkPermissionState(private)"}

/** Main-owned drafts, actual owner futures retain their own execution/lifetime. */
internal class NetworkPermissionController(
    private val scope:CoroutineScope,private val capture:suspend()->NetworkPermissionBackend,
    private val applied:suspend(NetworkPermissionBackend)->Unit,
){
    private val mutable=MutableStateFlow(NetworkPermissionState())
    val state=mutable.asStateFlow()
    private var backend:NetworkPermissionBackend?=null
    private var request:Job?=null
    private var serial=0L
    fun available()=backend?.current==true
    private fun current(epoch:Long)=mutable.value.open&&mutable.value.epoch==epoch
    fun edit(id:String){
        if(mutable.value.saving)return
        request?.cancel();backend=null
        check(serial<Long.MAX_VALUE)
        val epoch=++serial
        mutable.value=NetworkPermissionState(open=true,epoch=epoch,id=id,loading=true)
        request=scope.launch{
            try{
                val captured=capture();check(captured.current)
                val source=captured.source(id)
                if(!current(epoch))return@launch
                check(captured.current)
                require(source.id==id)
                backend=captured
                mutable.update{it.copy(source=source,internet=source.internet,lan=source.lan,loading=false,
                    problem=if(source.managed||!source.supported)NetworkPermissionProblem.Unsupported else null)}
            }catch(failure:CancellationException){throw failure}
            catch(_:Exception){if(current(epoch))mutable.update{it.copy(loading=false,problem=NetworkPermissionProblem.Unavailable)}}
        }
    }
    fun dismiss(epoch:Long){
        if(!current(epoch)||mutable.value.saving)return
        request?.cancel();backend=null
        mutable.value=NetworkPermissionState(epoch=epoch)
    }
    fun reload(epoch:Long){if(current(epoch)&&!mutable.value.saving)mutable.value.id?.let(::edit)}
    fun internet(epoch:Long,value:Boolean){change(epoch,value,mutable.value.lan)}
    fun lan(epoch:Long,value:Boolean){change(epoch,mutable.value.internet,value)}
    fun change(epoch:Long,internet:Boolean,lan:Boolean){
        val old=mutable.value;val source=old.source?:return
        if(!current(epoch)||old.busy||!available()||!source.supported||source.managed||source.status !in STOPPED)return
        if(!source.networkEnabled&&((internet&&!old.internet)||(lan&&!old.lan)))return
        mutable.value=old.copy(internet=internet,lan=lan,problem=null)
    }
    fun save(epoch:Long){
        val old=mutable.value;val source=old.source?:return;val owner=backend?:return
        if(!current(epoch)||old.busy||!owner.current||!source.supported||source.managed||source.status !in STOPPED)return
        if(!source.networkEnabled&&(old.internet||old.lan))return
        mutable.update{it.copy(saving=true,problem=null)}
        request=scope.launch{
            try{
                val actual=owner.source(source.id)
                check(owner.current)
                if(actual.identity!=source.identity||actual.version!=source.version||actual.managed||actual.status !in STOPPED){
                    if(current(epoch))mutable.update{it.copy(saving=false,problem=NetworkPermissionProblem.Changed)}
                    return@launch
                }
                owner.save(source,old.internet,old.lan)
                check(owner.current)
                applied(owner)
                if(current(epoch)){backend=null;mutable.value=NetworkPermissionState(epoch=epoch)}
            }catch(failure:CancellationException){throw failure}
            catch(_:Exception){if(current(epoch))mutable.update{it.copy(saving=false,
                problem=if(owner.current)NetworkPermissionProblem.Save else NetworkPermissionProblem.Unavailable)}}
        }
    }
    companion object{val STOPPED=setOf("created","exited","dead")}
}
