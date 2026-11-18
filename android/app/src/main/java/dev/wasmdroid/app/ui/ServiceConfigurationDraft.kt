package dev.wasmdroid.app.ui

import dev.wasmdroid.app.engine.PackageRuntimeRequirements
import org.json.JSONArray
import org.json.JSONObject

/** Private form state only. Never persist in saved state or include values in
 * error messages, events, analytics, operation receipts or debug strings. */
internal enum class ConfigurationField { Document,Name,Command,Arguments,Environment,Mounts,Ports,WorkingDirectory,Timeout,Network,Automation }
internal enum class AutomaticRestartPolicy(val wire:String) {
    Never("no"),OnFailure("on_failure"),Always("always");
    companion object {
        fun parse(value:Any):AutomaticRestartPolicy = entries.firstOrNull{it.wire==value}
            ?:throw ConfigurationInputError(ConfigurationField.Automation)
    }
}
internal class ConfigurationInputError(val field:ConfigurationField):IllegalArgumentException("Invalid configuration field: ${field.name}")
internal data class EnvironmentDraft(val name:String,val value:String,val required:Boolean=false){override fun toString()="EnvironmentDraft(private)"}
internal data class MountDraft(val volume:String,val target:String,val readOnly:Boolean){override fun toString()="MountDraft(private)"}
private const val STARTUP_PORT_LABEL = "dev.wasmdroid.startup.port"

internal data class PortDraft(
    val guest: String,
    val phone: String = "0",
    val protocol: String = "tcp",
    val web: Boolean = false,
    val startup: Boolean = false,
) { override fun toString() = "PortDraft(private)" }
internal data class ServiceConfigurationDraft(
    private val original:String,
    val name:String,val command:String,val arguments:List<String>,val environment:List<EnvironmentDraft>,
    val workingDirectory:String,val mounts:List<MountDraft>,val ports:List<PortDraft>,
    val network:String,val tty:Boolean,val stdinOpen:Boolean,val timeoutSeconds:String,
    val startOnBoot:Boolean=false,val restartPolicy:AutomaticRestartPolicy=AutomaticRestartPolicy.Never,
) {
    override fun toString()="ServiceConfigurationDraft(private)"
    fun textBytes():Long=(listOf(name,command,workingDirectory,timeoutSeconds,network,restartPolicy.wire)+arguments+
        environment.flatMap{listOf(it.name,it.value)}+mounts.flatMap{listOf(it.volume,it.target)}+
        ports.flatMap{listOf(it.guest,it.phone,it.protocol)}).sumOf{it.toByteArray(Charsets.UTF_8).size.toLong()}
    fun specification():String {
        requireField(name.toByteArray(Charsets.UTF_8).size<=160&&!name.any(Char::isISOControl),ConfigurationField.Name)
        requireField(command.isNotBlank()&&!command.contains('\u0000'),ConfigurationField.Command)
        requireField(arguments.size<=256&&arguments.none{it.contains('\u0000')},ConfigurationField.Arguments)
        requireField(environment.size<=128&&environment.map{it.name}.distinct().size==environment.size&&environment.all{
            it.name.isNotEmpty()&&!it.name.contains('=')&&!it.name.contains('\u0000')&&!it.value.contains('\u0000')&&
                (!it.required || PackageRuntimeRequirements.validVariable(it.name))},ConfigurationField.Environment)
        requireField(workingDirectory.isEmpty()||guestPath(workingDirectory),ConfigurationField.WorkingDirectory)
        requireField(mounts.size<=32&&mounts.map{it.target}.distinct().size==mounts.size&&mounts.all{
            Regex("[A-Za-z0-9_-]{1,128}").matches(it.volume)&&guestPath(it.target)},ConfigurationField.Mounts)
        requireField(ports.size<=16&&ports.map{it.guest}.distinct().size==ports.size,ConfigurationField.Ports)
        requireField(ports.count { it.web } <= 1 && ports.count { it.startup } <= 1, ConfigurationField.Ports)
        requireField(network in setOf("loopback","none")&&(network!="none"||ports.isEmpty()),ConfigurationField.Network)
        val portValues=ports.map{port->
            requireField(port.protocol=="tcp",ConfigurationField.Ports)
            val guest=number(port.guest,1,65535,ConfigurationField.Ports)
            val phone=number(port.phone.ifEmpty { "0" },0,65535,ConfigurationField.Ports)
            JSONObject().put("instance_port",guest).put("host_port",phone).put("protocol",port.protocol)
        }
        val timeout=if(timeoutSeconds.isEmpty())null else number(timeoutSeconds,1,Long.MAX_VALUE/1000,ConfigurationField.Timeout)
        val result=JSONObject(original)
        result.put("command",command).put("args",JSONArray(arguments))
            .put("env",JSONObject().apply{environment.forEach{put(it.name,it.value)}})
            .put("working_dir",if(workingDirectory.isEmpty())JSONObject.NULL else workingDirectory)
            .put("tty",tty).put("stdin_open",stdinOpen).put("network",network)
            .put("start_on_boot",startOnBoot).put("restart_policy",restartPolicy.wire)
            .put("mounts",JSONArray().apply{mounts.forEach{put(JSONObject()
                .put("source",JSONObject().put("type","volume").put("source",it.volume))
                .put("target",it.target).put("read_only",it.readOnly))}})
            .put("ports",JSONArray(portValues))
        val resources=result.optJSONObject("resources")?:JSONObject()
        resources.put("timeout_seconds",timeout?:JSONObject.NULL);result.put("resources",resources)
        val labels=result.optJSONObject("labels")?:JSONObject()
        if(name.isBlank())labels.remove("dev.wasmdroid.ui.name")else labels.put("dev.wasmdroid.ui.name",name)
        val required = environment.filter { it.required }.map { it.name }
        if (required.isEmpty()) labels.remove(PackageRuntimeRequirements.SERVICE_LABEL)
        else labels.put(PackageRuntimeRequirements.SERVICE_LABEL, JSONArray(required).toString())
        val webPort = ports.firstOrNull { it.web }?.guest
        if (webPort == null) labels.remove("dev.wasmdroid.web.port")
        else labels.put("dev.wasmdroid.web.port", webPort)
        val startupPort = ports.firstOrNull { it.startup }?.guest
        if (startupPort == null) labels.remove(STARTUP_PORT_LABEL)
        else labels.put(STARTUP_PORT_LABEL, startupPort)
        result.put("labels",labels)
        val serialized=result.toString()
        requireField(serialized.toByteArray(Charsets.UTF_8).size<=128*1024,ConfigurationField.Document)
        return serialized
    }
    companion object {
        /** Reject lossy/coercive conversion. Unknown root/runtime fields remain
         * in original for authoritative native validation; nested rows are strict. */
        fun fromSpecification(raw:String):ServiceConfigurationDraft {
            requireField(raw.toByteArray(Charsets.UTF_8).size<=128*1024,ConfigurationField.Document)
            try{
                val value=document(raw)
                val arguments=array(value,"args").let{items->(0 until items.length()).map{typedString(items.get(it))}}
                val env=if(value.has("env"))value.getJSONObject("env")else JSONObject()
                val labels=if(value.has("labels"))value.getJSONObject("labels")else JSONObject()
                val startupPort = if (labels.has(STARTUP_PORT_LABEL)) typedString(labels.get(STARTUP_PORT_LABEL)) else null
                val required = PackageRuntimeRequirements.manualNames(labels)
                val environment = env.keys().asSequence().map { name ->
                    EnvironmentDraft(name, typedString(env.get(name)), name in required)
                }.toList() + required.filterNot(env::has).map { EnvironmentDraft(it, "", required = true) }
                val resources=if(value.has("resources"))value.getJSONObject("resources")else JSONObject()
                val mounts=array(value,"mounts").let{items->(0 until items.length()).map{index->
                    val mount=items.getJSONObject(index);keys(mount,setOf("source","target","read_only"),ConfigurationField.Mounts)
                    val source=mount.getJSONObject("source");keys(source,setOf("type","source"),ConfigurationField.Mounts)
                    requireField(typedString(source.get("type"))=="volume",ConfigurationField.Mounts)
                    val readOnly=mount.get("read_only") as? Boolean?:throw ConfigurationInputError(ConfigurationField.Mounts)
                    MountDraft(typedString(source.get("source")),typedString(mount.get("target")),readOnly)
                }}
                val ports=array(value,"ports").let{items->(0 until items.length()).map{index->
                    val port=items.getJSONObject(index);keys(port,setOf("instance_port","host_port","protocol"),ConfigurationField.Ports)
                    val guest = integer(port.get("instance_port"))
                    val protocol = typedString(port.get("protocol"))
                    PortDraft(guest, integer(port.get("host_port")), protocol,
                        web = protocol == "tcp" && guest == labels.optString("dev.wasmdroid.web.port"),
                        startup = protocol == "tcp" && guest == startupPort)
                }}
                requireField(startupPort == null || ports.count { it.startup } == 1, ConfigurationField.Ports)
                val draft=ServiceConfigurationDraft(value.toString(),
                    if(labels.has("dev.wasmdroid.ui.name"))typedString(labels.get("dev.wasmdroid.ui.name"))else "",
                    typedString(value.get("command")),arguments,environment,
                    if(!value.has("working_dir")||value.isNull("working_dir"))"" else typedString(value.get("working_dir")),mounts,ports,
                    if(value.has("network"))typedString(value.get("network"))else "loopback",boolean(value,"tty"),boolean(value,"stdin_open"),
                    if(!resources.has("timeout_seconds")||resources.isNull("timeout_seconds"))""else integer(resources.get("timeout_seconds")),
                    if(!value.has("start_on_boot"))false else value.get("start_on_boot") as? Boolean?:throw ConfigurationInputError(ConfigurationField.Automation),
                    if(!value.has("restart_policy"))AutomaticRestartPolicy.Never else AutomaticRestartPolicy.parse(value.get("restart_policy")))
                // Refuse oversized form collections; the existing guarded JSON
                // mode remains available instead of silently truncating them.
                requireField(arguments.size<=256&&draft.environment.size<=128&&mounts.size<=32&&ports.size<=16,ConfigurationField.Document)
                return draft
            }catch(error:ConfigurationInputError){throw error}
            catch(_error:Exception){throw ConfigurationInputError(ConfigurationField.Document)}
        }
        /** Bound private raw-editor work before the platform JSON parser. */
        fun document(raw:String):JSONObject {
            requireField(raw.toByteArray(Charsets.UTF_8).size<=128*1024,ConfigurationField.Document)
            var quoted=false;var escaped=false;var depth=0;var tokens=0
            for(character in raw){
                if(quoted){
                    if(escaped)escaped=false else when(character){'\\'->escaped=true;'"'->quoted=false}
                }else when(character){
                    '"'->quoted=true
                    '{','['->{depth++;tokens++}
                    '}',']'->depth--
                    ',',':'->tokens++
                }
                requireField(depth in 0..32&&tokens<=16_384,ConfigurationField.Document)
            }
            requireField(!quoted&&!escaped&&depth==0,ConfigurationField.Document)
            return try{JSONObject(raw)}catch(_error:Exception){throw ConfigurationInputError(ConfigurationField.Document)}
        }
        private fun array(value:JSONObject,name:String)=if(value.has(name))value.getJSONArray(name)else JSONArray()
        private fun typedString(value:Any):String=value as? String?:throw ConfigurationInputError(ConfigurationField.Document)
        private fun integer(value:Any):String {
            requireField(value is Number&&Regex("0|[1-9][0-9]*").matches(value.toString()),ConfigurationField.Document)
            return value.toString()
        }
        private fun boolean(value:JSONObject,name:String):Boolean=if(!value.has(name))false else value.get(name) as? Boolean?:throw ConfigurationInputError(ConfigurationField.Document)
        private fun keys(value:JSONObject,allowed:Set<String>,field:ConfigurationField){requireField(value.keys().asSequence().all{it in allowed},field)}
        private fun number(value:String,min:Long,max:Long,field:ConfigurationField):Long {
            requireField(Regex("0|[1-9][0-9]*").matches(value),field)
            val parsed=value.toLongOrNull()?:throw ConfigurationInputError(field)
            requireField(parsed in min..max,field);return parsed
        }
        private fun guestPath(value:String)=value.startsWith('/')&&!value.contains('\\')&&!value.any(Char::isISOControl)&&
            (value=="/"||value.substring(1).split('/').none{it.isEmpty()||it=="."||it==".."})
        private fun requireField(condition:Boolean,field:ConfigurationField){if(!condition)throw ConfigurationInputError(field)}
    }
}
