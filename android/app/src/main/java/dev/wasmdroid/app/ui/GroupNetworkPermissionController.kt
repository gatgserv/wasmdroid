package dev.wasmdroid.app.ui

import kotlinx.coroutines.*
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.update

internal data class GroupPermissionMember(val id:String,val identity:String,val service:String,val version:ULong)
internal data class GroupPermissionSource(
    val target:String,val name:String,val version:ULong,val grantRevision:ULong,
    val members:List<GroupPermissionMember>,val internet:Boolean?,val lan:Boolean?,
    val canEdit:Boolean,val networkEnabled:Boolean,val supported:Boolean,
){override fun toString()="GroupPermissionSource(private)"}
internal interface GroupPermissionReview{val source:GroupPermissionSource}
internal interface GroupPermissionBackend{
    val current:Boolean
    suspend fun review(target:String):GroupPermissionReview
    suspend fun save(review:GroupPermissionReview,internet:Boolean,lan:Boolean)
}
internal data class GroupPermissionState(
    val open:Boolean=false,val epoch:Long=0,val target:String?=null,val source:GroupPermissionSource?=null,
    val internet:Boolean?=null,val lan:Boolean?=null,val loading:Boolean=false,val saving:Boolean=false,
    val problem:NetworkPermissionProblem?=null,
){val busy:Boolean get()=loading||saving;override fun toString()="GroupPermissionState(private)"}
/** A draft retains the actual opaque native review, never a reconstructed grant. */
internal class GroupNetworkPermissionController(
    private val scope:CoroutineScope,private val capture:suspend()->GroupPermissionBackend,
    private val applied:suspend(GroupPermissionBackend)->Unit,
){
    private val mutable=MutableStateFlow(GroupPermissionState());val state=mutable.asStateFlow()
    private var backend:GroupPermissionBackend?=null;private var review:GroupPermissionReview?=null
    private var request:Job?=null;private var serial=0L
    fun available()=backend?.current==true
    private fun current(epoch:Long)=mutable.value.open&&mutable.value.epoch==epoch
    fun edit(target:String){
        if(mutable.value.saving)return
        request?.cancel();backend=null;review=null;check(serial<Long.MAX_VALUE)
        val epoch=++serial;mutable.value=GroupPermissionState(open=true,epoch=epoch,target=target,loading=true)
        request=scope.launch{
            try{
                val owner=capture();check(owner.current);val actual=owner.review(target)
                if(!current(epoch))return@launch
                check(owner.current);backend=owner;review=actual;val source=actual.source
                mutable.update{it.copy(source=source,internet=source.internet,lan=source.lan,loading=false,
                    problem=if(!source.supported)NetworkPermissionProblem.Unsupported else null)}
            }catch(failure:CancellationException){throw failure}
            catch(_:Exception){if(current(epoch))mutable.update{it.copy(loading=false,problem=NetworkPermissionProblem.Unavailable)}}
        }
    }
    fun dismiss(epoch:Long){if(!current(epoch)||mutable.value.saving)return;request?.cancel();backend=null;review=null;mutable.value=GroupPermissionState(epoch=epoch)}
    fun reload(epoch:Long){if(current(epoch)&&!mutable.value.saving)mutable.value.target?.let(::edit)}
    fun choose(epoch:Long,internet:Boolean,value:Boolean){
        val old=mutable.value;val source=old.source?:return
        if(!current(epoch)||old.busy||!available()||!source.supported||!source.canEdit||(!source.networkEnabled&&value))return
        mutable.value=if(internet)old.copy(internet=value,problem=null)else old.copy(lan=value,problem=null)
    }
    fun save(epoch:Long){
        val old=mutable.value;val actual=review?:return;val owner=backend?:return;val source=actual.source
        val internet=old.internet?:return;val lan=old.lan?:return
        if(!current(epoch)||old.busy||!owner.current||!source.supported||!source.canEdit||(!source.networkEnabled&&(internet||lan)))return
        mutable.update{it.copy(saving=true,problem=null)}
        request=scope.launch{
            try{
                // Atomic native setter validates every listed member, birth,
                // grant revision and run reservation against this exact review.
                owner.save(actual,internet,lan);check(owner.current);applied(owner)
                if(current(epoch)){backend=null;review=null;mutable.value=GroupPermissionState(epoch=epoch)}
            }catch(failure:CancellationException){throw failure}
            catch(_:Exception){if(current(epoch))mutable.update{it.copy(saving=false,problem=if(owner.current)NetworkPermissionProblem.Changed else NetworkPermissionProblem.Unavailable)}}
        }
    }
}
