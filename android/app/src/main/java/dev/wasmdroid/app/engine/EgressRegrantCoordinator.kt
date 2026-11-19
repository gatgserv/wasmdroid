package dev.wasmdroid.app.engine

import java.util.concurrent.CompletableFuture
import java.util.concurrent.atomic.AtomicBoolean
import java.util.concurrent.atomic.AtomicReference

internal data class EgressCurrent(val grant:Long,val network:Long,val closed:Boolean,val uncertain:Boolean)
internal data class EgressAttachment(val current:EgressCurrent?,val ready:Boolean,val closed:Boolean,val uncertain:Boolean,val stopped:Boolean)
internal data class EgressReplacementStatus(val grant:Long,val network:Long,val admitted:Boolean,val terminal:Boolean,val closed:Boolean,val uncertain:Boolean)
internal interface EgressRegrantBackend<C,O>{
    fun attachment():EgressAttachment
    fun acquire():CompletableFuture<C>
    fun epoch(capture:C):Long
    fun release(capture:C)
    /** Failure here proves no manager was allocated. */
    fun begin(capture:C):O
    fun dispatch(operation:O)
    fun status(operation:O):EgressReplacementStatus
    fun cancel(operation:O)
}
/** One execution-loop owner; Stop only cancels the retained operation atomically.
 * A timeout/error never stands in for a native no-admission/Closed receipt. */
internal class EgressRegrantCoordinator<C:Any,O:Any>(private val backend:EgressRegrantBackend<C,O>,initial:C){
    private val stopped=AtomicBoolean(false)
    private val operation=AtomicReference<O?>(null)
    private var capture:C?=initial
    private var acquiring:CompletableFuture<C>?=null
    private var uncertain=false
    private var retryAt=0L
    fun stop(){stopped.set(true);operation.get()?.let{runCatching{backend.cancel(it)}}}
    fun tick(now:Long){
        if(stopped.get()||uncertain||now<retryAt)return
        try{
            val attached=backend.attachment()
            if(attached.uncertain){uncertain=true;return}
            if(attached.stopped){stop();return}
            val pending=operation.get()
            if(pending!=null){
                val result=backend.status(pending);val owned=capture?:error("Missing retained capture")
                check(result.network==backend.epoch(owned))
                if(result.uncertain){uncertain=true;return}
                if(!result.terminal)return
                if(result.admitted){
                    val current=attached.current?:return
                    if(current.grant!=result.grant||current.network!=result.network)return
                    operation.compareAndSet(pending,null)
                }else{
                    if(!result.closed)return
                    backend.release(owned);capture=null;operation.compareAndSet(pending,null)
                    retryAt=now+1_000;return
                }
            }
            capture?.let{owned->
                val current=attached.current?:return
                check(current.network==backend.epoch(owned))
                if(current.uncertain){uncertain=true;return}
                if(!current.closed)return
                backend.release(owned);capture=null
            }
            if(!attached.ready||!attached.closed||attached.uncertain||stopped.get())return
            var future=acquiring
            if(future==null){future=backend.acquire();acquiring=future}
            if(!future.isDone)return
            val fresh=try{future.join()}catch(_:Exception){acquiring=null;retryAt=now+1_000;return}
            acquiring=null
            if(stopped.get()){backend.release(fresh);return}
            val reserved=try{backend.begin(fresh)}catch(_:Exception){backend.release(fresh);retryAt=now+1_000;return}
            capture=fresh;operation.set(reserved)
            // Main can stop between publication and dispatch. The native
            // cancellation gate is permanent for this exact retained ticket.
            if(stopped.get())backend.cancel(reserved)
            runCatching{backend.dispatch(reserved)} // Keep its receipt on failure.
        }catch(_:Exception){
            // Unknown status cannot prove closure. Retain the native operation
            // and capture until worker teardown; never allocate a successor.
            uncertain=true
        }
    }
}
