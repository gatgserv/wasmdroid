package dev.wasmdroid.app.ui

import java.time.Instant
import kotlinx.coroutines.*
import kotlinx.coroutines.flow.*

data class RegistrySearchRow(
    val reference: String,
    val json: String,
    val releasedAt: Instant? = null,
    val webcVersion: Int? = null,
) {
    val legacyWebcVersion: Int? get() = webcVersion?.takeIf { it in 1..2 }
}
internal data class RegistrySearchPage(val rows:List<RegistrySearchRow>,val nextCursor:String?)
enum class RegistrySearchPhase { Idle, LoadingFirst, LoadingNext, Ready, Failed, Cancelled, Unavailable }
enum class RegistrySearchProblem { Failed, InvalidQuery, InvalidPage, LimitReached }
enum class RegistrySearchOrder { Registry, NewestRelease }
data class RegistrySearchState(
    val draftQuery:String="",val requestedQuery:String?=null,val resultsQuery:String?=null,
    val rows:List<RegistrySearchRow> = emptyList(),val nextCursor:String?=null,
    val phase:RegistrySearchPhase=RegistrySearchPhase.Idle,val problem:RegistrySearchProblem?=null,
    val ownerConnected:Boolean=false,val requestEpoch:Long=0,
    val pages:Int=0,val retryAvailable:Boolean=false,
    val order: RegistrySearchOrder = RegistrySearchOrder.Registry,
    val showIncompatible: Boolean = false,
) {
    val visibleRows: List<RegistrySearchRow>
        get() = if (showIncompatible) rows else rows.filter { it.legacyWebcVersion == null }
    val loading get()=phase==RegistrySearchPhase.LoadingFirst||phase==RegistrySearchPhase.LoadingNext
    val empty get()=phase==RegistrySearchPhase.Ready&&rows.isEmpty()
    val showingPreviousResults get()=resultsQuery!=null&&(resultsQuery!=draftQuery||phase in setOf(
        RegistrySearchPhase.LoadingFirst,RegistrySearchPhase.Failed,RegistrySearchPhase.Cancelled,RegistrySearchPhase.Unavailable))
    val canNext get()=ownerConnected&&!loading&&phase==RegistrySearchPhase.Ready&&nextCursor!=null
    val canRetry get()=ownerConnected&&!loading&&retryAvailable&&phase in setOf(
        RegistrySearchPhase.Failed,RegistrySearchPhase.Cancelled,RegistrySearchPhase.Unavailable)
    override fun toString()="RegistrySearchState(private)"
}
internal interface RegistrySearchBackend {
    val current:Boolean
    suspend fun search(query:String,cursor:String?):RegistrySearchPage
}

/** Main-owner-confined state. No persistent history, network observation or auto
 * retry. Cancelling ends this observation; it does not claim HTTP cancellation. */
internal class RegistrySearchController(
    private val scope:CoroutineScope,private val capture:suspend()->RegistrySearchBackend,
    private val onCurrentFailure:(Exception)->Unit={},
) : AutoCloseable {
    private class Request(val query:String,val cursor:String?,val epoch:Long)
    private val mutable=MutableStateFlow(RegistrySearchState())
    val state:StateFlow<RegistrySearchState> = mutable.asStateFlow()
    private var serial=0L
    private var current:Request?=null
    private var retryRequest:Request?=null
    private var job:Job?=null
    private var closed=false
    private var consumedCursors=emptySet<String>()

    fun edit(query:String){if(!closed)mutable.update{it.copy(draftQuery=query.take(MAX_QUERY_CHARS))}}
    fun setOrder(order: RegistrySearchOrder) {
        if (!closed) mutable.update { it.copy(order = order) }
    }
    fun setShowIncompatible(show: Boolean) {
        if (!closed) mutable.update { it.copy(showIncompatible = show) }
    }
    fun availability(ownerConnected:Boolean){
        if(closed)return
        val old=mutable.value
        if(!ownerConnected){
            if(old.loading)retryRequest=current
            else if(retryRequest==null)old.resultsQuery?.let{retryRequest=Request(it,null,serial)}
            invalidate()
            mutable.value=old.copy(ownerConnected=false,phase=RegistrySearchPhase.Unavailable,requestEpoch=serial,retryAvailable=retryRequest!=null)
        }else mutable.update{it.copy(ownerConnected=true,
            phase=if(it.phase==RegistrySearchPhase.Unavailable&&it.requestedQuery==null)RegistrySearchPhase.Idle else it.phase)}
    }
    fun search(query:String=mutable.value.draftQuery){
        if(closed)return
        if(query.isBlank()||query.length>MAX_QUERY_CHARS){
            invalidate();retryRequest=null
            mutable.update{it.copy(requestedQuery=null,phase=RegistrySearchPhase.Failed,problem=RegistrySearchProblem.InvalidQuery,requestEpoch=serial,retryAvailable=false)}
            return
        }
        mutable.update{it.copy(draftQuery=query)}
        begin(query,null)
    }
    fun next(){
        val old=mutable.value
        if(closed||!old.canNext)return
        if(old.pages>=MAX_PAGES){retryRequest=null;mutable.update{it.copy(phase=RegistrySearchPhase.Failed,problem=RegistrySearchProblem.LimitReached,retryAvailable=false)};return}
        begin(checkNotNull(old.resultsQuery),checkNotNull(old.nextCursor))
    }
    fun retry(){
        if(closed||!mutable.value.canRetry)return
        retryRequest?.let{begin(it.query,it.cursor)}
    }
    fun cancel(){
        if(closed||!mutable.value.loading)return
        retryRequest=current;invalidate()
        mutable.update{it.copy(phase=RegistrySearchPhase.Cancelled,problem=null,requestEpoch=serial,retryAvailable=retryRequest!=null)}
    }
    private fun invalidate(){
        check(serial<Long.MAX_VALUE){"Registry search epoch exhausted"};serial++
        current=null;job?.cancel();job=null
    }
    private fun active(request:Request)=!closed&&current===request&&mutable.value.requestEpoch==request.epoch&&mutable.value.ownerConnected
    private fun begin(query:String,cursor:String?){
        invalidate()
        val request=Request(query,cursor,serial)
        retryRequest=request
        if(!mutable.value.ownerConnected){mutable.update{it.copy(requestedQuery=query,phase=RegistrySearchPhase.Unavailable,problem=null,requestEpoch=serial,retryAvailable=true)};return}
        current=request
        mutable.update{it.copy(requestedQuery=query,phase=if(cursor==null)RegistrySearchPhase.LoadingFirst else RegistrySearchPhase.LoadingNext,problem=null,requestEpoch=serial,retryAvailable=true)}
        job=scope.launch{
            var backend:RegistrySearchBackend?=null
            try{
                backend=capture()
                if(!active(request))return@launch
                if(!backend.current){unavailable(request);return@launch}
                val page=backend.search(query,cursor)
                if(!active(request))return@launch
                if(!backend.current){unavailable(request);return@launch}
                validate(page)
                val seen=if(cursor==null)emptySet() else consumedCursors
                if(page.nextCursor!=null&&(page.nextCursor==cursor||page.nextCursor in seen))throw InvalidPage()
                val rows=linkedMapOf<String,RegistrySearchRow>()
                if(cursor!=null)mutable.value.rows.forEach{rows[it.reference]=it}
                page.rows.forEach{rows.putIfAbsent(it.reference,it)}
                if(rows.size>MAX_ROWS||rows.values.sumOf{it.json.toByteArray(Charsets.UTF_8).size.toLong()}>MAX_RETAINED_BYTES)throw LimitReached()
                consumedCursors=if(cursor==null)emptySet() else seen+cursor
                current=null;retryRequest=null
                mutable.update{it.copy(resultsQuery=query,rows=rows.values.toList(),nextCursor=page.nextCursor,
                    phase=RegistrySearchPhase.Ready,problem=null,pages=if(cursor==null)1 else it.pages+1,retryAvailable=false)}
            }catch(failure:CancellationException){
                if(active(request)){current=null;mutable.update{it.copy(phase=RegistrySearchPhase.Cancelled,problem=null)}}
            }catch(failure:Exception){
                if(active(request)){
                    if(backend?.current==false){unavailable(request);return@launch}
                    current=null
                    mutable.update{it.copy(phase=RegistrySearchPhase.Failed,problem=when(failure){is InvalidPage->RegistrySearchProblem.InvalidPage;is LimitReached->RegistrySearchProblem.LimitReached;else->RegistrySearchProblem.Failed})}
                    onCurrentFailure(failure)
                }
            }
        }
    }
    private fun unavailable(request:Request){
        if(active(request)){current=null;mutable.update{it.copy(ownerConnected=false,phase=RegistrySearchPhase.Unavailable,problem=null)}}
    }
    private fun validate(page:RegistrySearchPage){
        if(page.rows.size>MAX_PAGE_ROWS||page.nextCursor?.let{it.isBlank()||it.toByteArray(Charsets.UTF_8).size>MAX_CURSOR_BYTES}==true)throw InvalidPage()
        if(page.rows.any{it.reference.isBlank()||it.reference.length>1024||it.json.toByteArray(Charsets.UTF_8).size>MAX_ROW_BYTES})throw InvalidPage()
    }
    override fun close(){if(!closed){invalidate();closed=true;retryRequest=null;mutable.value=RegistrySearchState(requestEpoch=serial)}}
    private class InvalidPage:Exception("Invalid registry search page")
    private class LimitReached:Exception("Registry search result limit reached")
    companion object {
        const val MAX_QUERY_CHARS=256
        const val MAX_PAGE_ROWS=30 // Includes the optional exact release on the first page.
        const val MAX_ROWS=240
        const val MAX_PAGES=32
        const val MAX_CURSOR_BYTES=4096
        const val MAX_ROW_BYTES=16*1024
        const val MAX_PAGE_BYTES=512*1024
        const val MAX_RETAINED_BYTES=1024*1024L
    }
}
