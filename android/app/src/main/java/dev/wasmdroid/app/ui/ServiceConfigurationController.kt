package dev.wasmdroid.app.ui

import kotlinx.coroutines.*
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.update
import org.json.JSONObject

internal interface ConfigurationBackend {
    val current:Boolean
    suspend fun inspect(id:String):String
    suspend fun group(id:String):String?
    suspend fun create(spec:String):String
    suspend fun configure(id:String,version:ULong,spec:String):String
    suspend fun createVolume(name:String):String
}
internal enum class ConfigurationProblem { Input,Unavailable,Changed,Unsupported,Busy,Save,Volume,VolumeName }
internal data class ConfigurationState(
    val open:Boolean=false,val epoch:Long=0,val instanceId:String?=null,val version:ULong?=null,
    val reference:String="",val endpoint:String="local",val sourceState:String="created",
    val groupId:String?=null,
    val draft:ServiceConfigurationDraft?=null,val raw:String="",val tab:Int=0,
    val loading:Boolean=false,val saving:Boolean=false,val makingVolume:Boolean=false,
    val volumeName:String="",val problem:ConfigurationProblem?=null,val field:ConfigurationField?=null,
){
    val busy:Boolean get()=loading||saving||makingVolume
    override fun toString()="ConfigurationState(private)"
}
/** Main-thread private editor. Native owner futures keep their own lifetime;
 * cancelling this ViewModel never cancels or fabricates their completion. */
internal class ServiceConfigurationController(
    private val scope:CoroutineScope,
    private val capture:suspend()->ConfigurationBackend,
    private val applied:suspend(ConfigurationBackend)->Unit,
) {
    private val mutable=MutableStateFlow(ConfigurationState())
    val state=mutable.asStateFlow()
    private var connection:ConfigurationBackend?=null
    private var request:Job?=null
    private var serial=0L
    fun available()=connection?.current==true
    fun create(raw:String){if(!mutable.value.saving&&!mutable.value.makingVolume)open(null,raw)}
    fun edit(id:String, initialTab:Int=0){if(!mutable.value.saving&&!mutable.value.makingVolume)open(id,null,initialTab)}
    private fun open(id:String?,initial:String?, initialTab:Int=0){
        request?.cancel();connection=null
        val epoch=++serial
        mutable.value=ConfigurationState(open=true,epoch=epoch,instanceId=id,loading=true)
        request=scope.launch{
            try{
                val backend=capture()
                check(backend.current){"Configuration connection changed"}
                val record=id?.let{JSONObject(backend.inspect(it))}
                val group=id?.let{backend.group(it)}
                val raw=initial?:record!!.getJSONObject("spec").toString()
                val value=ServiceConfigurationDraft.document(raw)
                val reference=value.get("artifact_ref") as? String?:throw ConfigurationInputError(ConfigurationField.Document)
                val endpoint=if(value.has("endpoint_id"))value.get("endpoint_id") as? String?:throw ConfigurationInputError(ConfigurationField.Document) else "local"
                val version=record?.getLong("version")?.also{require(it>=0)}?.toULong()
                val parsed=runCatching{ServiceConfigurationDraft.fromSpecification(raw)}
                if(current(epoch)&&backend.current){
                    connection=backend
                    mutable.value=ConfigurationState(open=true,epoch=epoch,instanceId=id,version=version,
                        reference=reference,endpoint=endpoint,sourceState=record?.getString("state")?:"created",
                        groupId=group,
                        draft=parsed.getOrNull(),raw=raw,tab=if(parsed.isSuccess)initialTab else 3,
                        problem=if(parsed.isSuccess)null else ConfigurationProblem.Input,
                        field=(parsed.exceptionOrNull() as? ConfigurationInputError)?.field)
                }else if(current(epoch))problem(epoch,ConfigurationProblem.Unavailable)
            }catch(failure:Exception){if(failure is CancellationException)throw failure;problem(epoch,classify(failure))}
            finally{if(current(epoch))mutable.update{it.copy(loading=false)}}
        }
    }
    fun reload(epoch:Long){
        if(!current(epoch)||mutable.value.busy)return
        val target=mutable.value.instanceId
        if(target!=null){open(target,null,mutable.value.tab.takeUnless { it == 3 } ?: 0);return}
        mutable.update{it.copy(loading=true,problem=null,field=null)}
        request=scope.launch{
            try{val backend=capture();check(backend.current);if(current(epoch)){connection=backend;mutable.update{it.copy(loading=false)}}}
            catch(failure:Exception){if(failure is CancellationException)throw failure;problem(epoch,ConfigurationProblem.Unavailable)}
            finally{if(current(epoch))mutable.update{it.copy(loading=false)}}
        }
    }
    fun change(epoch:Long,update:(ServiceConfigurationDraft)->ServiceConfigurationDraft){
        val old=mutable.value
        if(!current(epoch)||old.busy||old.tab==3||old.draft==null||old.groupId!=null)return
        val changed=update(old.draft)
        if(changed.textBytes()>128*1024){inputError(epoch,ConfigurationField.Document);return}
        mutable.value=old.copy(draft=changed,problem=null,field=null)
    }
    fun raw(epoch:Long,value:String){
        if(!current(epoch)||mutable.value.busy||mutable.value.groupId!=null)return
        if(value.toByteArray(Charsets.UTF_8).size>128*1024){inputError(epoch,ConfigurationField.Document);return}
        mutable.update{it.copy(raw=value,problem=null,field=null)}
    }
    fun tab(epoch:Long,index:Int){
        val old=mutable.value
        if(!current(epoch)||old.busy||index !in 0..3||index==old.tab)return
        try{
            mutable.value=when{
                index==3->old.copy(raw=old.draft!!.specification(),tab=index,problem=null,field=null)
                old.tab==3->{identity(old,old.raw);old.copy(draft=ServiceConfigurationDraft.fromSpecification(old.raw),tab=index,problem=null,field=null)}
                else->old.copy(tab=index,problem=null,field=null)
            }
        }catch(failure:ConfigurationInputError){inputError(epoch,failure.field)}
    }
    fun volumeName(epoch:Long,name:String){
        if(!current(epoch)||mutable.value.busy||mutable.value.groupId!=null)return
        if(name.length>128){problem(epoch,ConfigurationProblem.VolumeName);return}
        mutable.update{it.copy(volumeName=name,problem=null,field=null)}
    }
    fun createVolume(epoch:Long){
        val old=mutable.value;val backend=connection
        if(!current(epoch)||old.busy||old.draft==null)return
        if(old.groupId!=null){problem(epoch,ConfigurationProblem.Unsupported);return}
        if(backend?.current!=true){problem(epoch,ConfigurationProblem.Unavailable);return}
        if(!Regex("[A-Za-z0-9_-]{1,128}").matches(old.volumeName)){problem(epoch,ConfigurationProblem.VolumeName);return}
        if(old.draft.mounts.size>=32){inputError(epoch,ConfigurationField.Mounts);return}
        mutable.update{it.copy(makingVolume=true,problem=null,field=null)}
        request=scope.launch{
            try{
                if(groupChanged(epoch,old,backend))return@launch
                check(backend.current){"Configuration connection changed"}
                val name=JSONObject(backend.createVolume(old.volumeName)).getString("name")
                if(current(epoch)){
                    val paths=old.draft.mounts.map{it.target}.toSet();var path="/data";var index=2
                    while(path in paths){path="/data${index++}"}
                    mutable.update{it.copy(makingVolume=false,volumeName="",draft=old.draft.copy(mounts=old.draft.mounts+MountDraft(name,path,false)))}
                }
                notifyApplied(backend)
            }catch(failure:Exception){if(failure is CancellationException)throw failure;problem(epoch,ConfigurationProblem.Volume)}
            finally{if(current(epoch))mutable.update{it.copy(makingVolume=false)}}
        }
    }
    fun save(epoch:Long){
        val old=mutable.value;val backend=connection
        if(!current(epoch)||old.busy)return
        if(old.groupId!=null){problem(epoch,ConfigurationProblem.Unsupported);return}
        if(backend?.current!=true){problem(epoch,ConfigurationProblem.Unavailable);return}
        if(old.tab!=3&&old.draft==null){inputError(epoch,ConfigurationField.Document);return}
        val spec=try{(if(old.tab==3)old.raw else old.draft!!.specification()).also{identity(old,it)}}
        catch(failure:ConfigurationInputError){inputError(epoch,failure.field);return}
        mutable.update{it.copy(saving=true,problem=null,field=null)}
        request=scope.launch{
            try{
                if(groupChanged(epoch,old,backend))return@launch
                check(backend.current){"Configuration connection changed"}
                if(old.instanceId==null)backend.create(spec)else backend.configure(old.instanceId,old.version!!,spec)
                // Actual native acknowledgement wins even if the UI connection
                // changed meanwhile; never offer a duplicate create/retry.
                if(current(epoch)){connection=null;mutable.value=ConfigurationState(epoch=++serial)}
                notifyApplied(backend)
            }catch(failure:Exception){if(failure is CancellationException)throw failure;problem(epoch,classify(failure))}
            finally{if(current(epoch))mutable.update{it.copy(saving=false)}}
        }
    }
    fun dismiss(epoch:Long){if(current(epoch)&&!mutable.value.saving&&!mutable.value.makingVolume)close()}
    fun close(){serial++;request?.cancel();request=null;connection=null;mutable.value=ConfigurationState(epoch=serial)}
    private fun current(epoch:Long)=mutable.value.open&&mutable.value.epoch==epoch
    private suspend fun groupChanged(epoch:Long,old:ConfigurationState,backend:ConfigurationBackend):Boolean {
        val group=old.instanceId?.let{backend.group(it)}?:return false
        check(backend.current){"Configuration connection is unavailable"}
        if(current(epoch))mutable.update{it.copy(groupId=group,problem=ConfigurationProblem.Unsupported,field=null)}
        return true
    }
    private fun identity(old:ConfigurationState,raw:String){
        val value=ServiceConfigurationDraft.document(raw)
        val endpoint=if(value.has("endpoint_id"))value.get("endpoint_id")else "local"
        if(value.opt("artifact_ref")!=old.reference||endpoint!=old.endpoint)throw ConfigurationInputError(ConfigurationField.Document)
    }
    private fun inputError(epoch:Long,field:ConfigurationField){
        if(current(epoch))mutable.update{it.copy(problem=ConfigurationProblem.Input,field=field,
            tab=if(it.tab==3||field==ConfigurationField.Document)it.tab else when(field){ConfigurationField.Arguments,ConfigurationField.Environment->1;ConfigurationField.Mounts,ConfigurationField.Ports,ConfigurationField.Network->2;else->0})}
    }
    private fun problem(epoch:Long,problem:ConfigurationProblem){if(current(epoch))mutable.update{it.copy(problem=problem,field=null)}}
    private suspend fun notifyApplied(backend:ConfigurationBackend){
        try{applied(backend)}catch(failure:Exception){if(failure is CancellationException)throw failure}
    }
    private fun classify(failure:Throwable):ConfigurationProblem {
        if(failure is ConfigurationInputError)return ConfigurationProblem.Input
        var actual=failure;repeat(8){actual.cause?.let{actual=it}}
        val message=actual.message.orEmpty()
        return when{
            message.contains("closing",true)||message.contains("connection",true)->ConfigurationProblem.Unavailable
            message.contains("stale",true)||message.contains("not found",true)||message.contains("changed",true)->ConfigurationProblem.Changed
            message.contains("unsupported",true)->ConfigurationProblem.Unsupported
            message.contains("stop",true)||message.contains("active",true)||message.contains("lease",true)||message.contains("locked",true)->ConfigurationProblem.Busy
            else->ConfigurationProblem.Save
        }
    }
}
