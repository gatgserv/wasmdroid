package dev.wasmdroid.app.ui

import kotlinx.coroutines.*
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import org.json.JSONArray
import org.json.JSONObject
import org.json.JSONTokener

enum class StorageUsagePhase { Idle, Loading, Ready, Failed, Unavailable }
enum class StorageCountStatus(val wire:String) { Complete("complete"), Partial("partial"), Unavailable("unavailable") }
enum class StorageCategory(val wire:String) {
    Artifacts("artifacts"), CompiledCache("compiled_cache"), Volumes("volumes"), Logs("logs")
}
enum class StorageUsageIssue(val wire:String) {
    PlatformUnsupported("platform_unsupported"), EntryLimit("entry_limit"), DepthLimit("depth_limit"),
    TimeLimit("time_limit"), Cancelled("cancelled"), UnsafeEntry("unsafe_entry"),
    PermissionDenied("permission_denied"), EntryChanged("entry_changed"), Io("io"), Overflow("overflow")
}
data class StorageCategoryUsage(
    val category:StorageCategory, val status:StorageCountStatus, val bytes:ULong?,
    val filesCounted:Int, val entriesVisited:Int, val issues:Set<StorageUsageIssue>,
)
data class StorageUsageSnapshot(
    val categories:List<StorageCategoryUsage>, val complete:Boolean, val observedTotalBytes:ULong?,
    val availableBytes:ULong?, val entriesVisited:Int, val issues:Set<StorageUsageIssue>,
)
data class StorageUsageState(
    val phase:StorageUsagePhase=StorageUsagePhase.Idle,
    val snapshot:StorageUsageSnapshot?=null, val requestEpoch:Long=0,
)

internal interface StorageUsageBackend {
    val current:Boolean
    suspend fun snapshot():StorageUsageSnapshot
}

/** Main-owner confined, non-saveable observations. Cancelling this view's
 * coroutine never acknowledges cancellation of the owner's retained scan. */
internal class StorageUsageController(
    private val scope:CoroutineScope,
    private val capture:suspend()->StorageUsageBackend,
) : AutoCloseable {
    private val mutable=MutableStateFlow(StorageUsageState())
    val state:StateFlow<StorageUsageState> = mutable.asStateFlow()
    private var connected=false
    private var closed=false
    private var serial=0L
    private var read:Job?=null

    fun availability(connected:Boolean) {
        if(closed)return
        this.connected=connected
        if(!connected){invalidate();mutable.value=StorageUsageState(StorageUsagePhase.Unavailable,requestEpoch=serial)}
        // Reconnection never fetches or revives an old observation implicitly.
    }
    fun refresh() {
        if(closed||mutable.value.phase==StorageUsagePhase.Loading)return
        if(!connected){mutable.value=StorageUsageState(StorageUsagePhase.Unavailable,requestEpoch=serial);return}
        val epoch=++serial
        mutable.value=StorageUsageState(StorageUsagePhase.Loading,requestEpoch=epoch)
        read=scope.launch {
            var backend:StorageUsageBackend?=null
            try {
                val selected=capture();backend=selected
                if(!valid(epoch))return@launch
                if(!selected.current){unavailable(epoch);return@launch}
                val snapshot=selected.snapshot()
                if(!valid(epoch))return@launch
                if(!selected.current){unavailable(epoch);return@launch}
                mutable.value=StorageUsageState(StorageUsagePhase.Ready,snapshot,epoch)
            } catch(failure:Exception) {
                if(valid(epoch))mutable.value=StorageUsageState(
                    if(failure is CancellationException||backend?.current==false)StorageUsagePhase.Unavailable else StorageUsagePhase.Failed,
                    requestEpoch=epoch)
                if(failure is CancellationException)throw failure
            } finally {
                if(valid(epoch)&&mutable.value.phase==StorageUsagePhase.Loading)unavailable(epoch)
            }
        }
    }
    private fun valid(epoch:Long)=!closed&&connected&&serial==epoch
    private fun unavailable(epoch:Long){mutable.value=StorageUsageState(StorageUsagePhase.Unavailable,requestEpoch=epoch)}
    private fun invalidate(){++serial;read?.cancel();read=null}
    override fun close(){if(closed)return;closed=true;connected=false;invalidate();mutable.value=StorageUsageState(StorageUsagePhase.Unavailable,requestEpoch=serial)}
}

/** English binary-unit formatting without a signed narrowing conversion or
 * floating-point loss. The largest remainder multiplication is <11*2^60. */
fun formatStorageBytes(bytes:ULong):String {
    val units=arrayOf("B","KiB","MiB","GiB","TiB","PiB","EiB")
    var unit=1uL;var index=0
    while(index<units.lastIndex&&bytes/unit>=1024uL){unit*=1024uL;index++}
    if(index==0)return "$bytes B"
    var whole=bytes/unit
    var tenth=((bytes%unit)*10uL+unit/2uL)/unit
    if(tenth==10uL){whole++;tenth=0uL}
    return "$whole.$tenth ${units[index]}"
}

/** Android JSON codec is deliberately separate from the pure typed controller.
 * JSONObject's lossy Double representation is refused, never cast to ULong. */
internal object StorageUsageCodec {
    fun parse(raw:String):StorageUsageSnapshot {
        require(raw.length<=16*1024&&raw.toByteArray(Charsets.UTF_8).size<=16*1024)
        require('\u0000' !in raw)
        boundedStructure(raw)
        val input=JSONTokener(raw)
        val json=input.nextValue().let{require(it is JSONObject);it}
        require(input.nextClean()=='\u0000'&&!input.more())
        keys(json,setOf("schema","metric","consistency","categories","complete","observed_total_bytes",
            "entries_visited","issues","available_bytes","available_status"))
        require(uint(json.get("schema"))==1uL)
        require(string(json,"metric")=="logical_regular_file_bytes")
        require(string(json,"consistency")=="best_effort_non_atomic")
        val rows=json.getJSONArray("categories");require(rows.length()==4)
        val categories=(0 until rows.length()).map { index ->
            val row=rows.getJSONObject(index)
            keys(row,setOf("category","status","logical_regular_file_bytes","files_counted","entries_visited","issues"))
            val category=StorageCategory.entries.singleOrNull{it.wire==string(row,"category")}?:error("Invalid storage category")
            val status=StorageCountStatus.entries.singleOrNull{it.wire==string(row,"status")}?:error("Invalid storage status")
            val bytes=optionalUint(row,"logical_regular_file_bytes")
            val files=count(row,"files_counted");val visited=count(row,"entries_visited")
            val issues=issues(row.getJSONArray("issues"))
            require(files<=visited)
            require(files!=0||bytes==null||bytes==0uL)
            when(status){
                StorageCountStatus.Complete->require(bytes!=null&&issues.isEmpty())
                StorageCountStatus.Partial->require(issues.isNotEmpty()&&(bytes!=null||StorageUsageIssue.Overflow in issues))
                StorageCountStatus.Unavailable->require(bytes==null&&files==0&&visited==0&&issues.isNotEmpty())
            }
            StorageCategoryUsage(category,status,bytes,files,visited,issues)
        }
        require(categories.map{it.category}.toSet()==StorageCategory.entries.toSet())
        val visited=count(json,"entries_visited")
        require(categories.sumOf{it.entriesVisited}==visited)
        val total=optionalUint(json,"observed_total_bytes")
        var computed:ULong?=0uL
        for(category in categories){
            val previous=computed;val next=category.bytes
            computed=if(previous==null||next==null||ULong.MAX_VALUE-previous<next)null else previous+next
        }
        require(total==computed)
        val globalIssues=issues(json.getJSONArray("issues"))
        val overflow=computed==null&&categories.all{it.bytes!=null}
        require(globalIssues==if(overflow)setOf(StorageUsageIssue.Overflow)else emptySet())
        val complete=boolean(json,"complete")
        require(complete==(globalIssues.isEmpty()&&categories.all{it.status==StorageCountStatus.Complete}))
        val available=optionalUint(json,"available_bytes")
        require(string(json,"available_status")==if(available==null)"unavailable" else "available")
        return StorageUsageSnapshot(categories.sortedBy{it.category.ordinal}.toList(),complete,total,available,visited,globalIssues)
    }
    private fun keys(json:JSONObject,expected:Set<String>){require(json.keys().asSequence().toSet()==expected)}
    private fun string(json:JSONObject,key:String):String=json.get(key).let{require(it is String);it}
    private fun boolean(json:JSONObject,key:String):Boolean=json.get(key).let{require(it is Boolean);it}
    private fun uint(value:Any):ULong {
        require(value is Int||value is Long){"Storage count requires an exact supported integer"}
        val number=(value as Number).toLong();require(number>=0)
        return number.toULong()
    }
    private fun optionalUint(json:JSONObject,key:String):ULong?=if(json.isNull(key))null else uint(json.get(key))
    private fun count(json:JSONObject,key:String):Int=uint(json.get(key)).also{require(it<=65_536uL)}.toInt()
    private fun issues(array:JSONArray):Set<StorageUsageIssue> {
        require(array.length()<=StorageUsageIssue.entries.size)
        val values=(0 until array.length()).map{index->
            val value=array.get(index);require(value is String)
            StorageUsageIssue.entries.singleOrNull{it.wire==value}?:error("Invalid storage outcome")
        }
        require(values.toSet().size==values.size)
        return values.toSet()
    }
    private fun boundedStructure(raw:String){
        var depth=0;var quoted=false;var escaped=false
        for(character in raw){
            if(quoted){if(escaped)escaped=false else if(character=='\\')escaped=true else if(character=='"')quoted=false}
            else when(character){'"'->quoted=true;'[','{'->{depth++;require(depth<=8)};']','}'->{depth--;require(depth>=0)}}
        }
        require(depth==0&&!quoted)
    }
}
