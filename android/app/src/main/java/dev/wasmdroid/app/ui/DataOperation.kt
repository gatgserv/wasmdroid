package dev.wasmdroid.app.ui

import dev.wasmdroid.engine.DataTransferControl
import kotlinx.coroutines.Job
import kotlinx.coroutines.CancellationException
import java.util.concurrent.ConcurrentLinkedDeque
import java.util.concurrent.Executors
import java.util.concurrent.atomic.AtomicBoolean

/** Cancellation closes native capabilities before possibly slow document-provider
 * streams. A blocked provider must not retain app volume ownership indefinitely. */
internal class DataOperation(val control:DataTransferControl){
    var job:Job?=null
    private val cancelled=AtomicBoolean(false)
    val isCancelled:Boolean get()=cancelled.get()
    private val native=ConcurrentLinkedDeque<AutoCloseable>()
    private val external=ConcurrentLinkedDeque<AutoCloseable>()
    fun <T:AutoCloseable> native(value:T):T=own(value,native)
    fun <T:AutoCloseable> external(value:T):T=own(value,external)
    private fun <T:AutoCloseable> own(value:T,list:ConcurrentLinkedDeque<AutoCloseable>):T{
        list.addFirst(value)
        if(cancelled.get()){runCatching{value.close()};throw CancellationException("Data transfer cancelled")}
        return value
    }
    fun cancel(){
        if(cancelled.compareAndSet(false,true)){
            control.cancel()
            cleanup.execute{closeResources()}
        }
    }
    private fun closeResources(){
        while(true){val item=native.pollFirst()?:break;runCatching{item.close()}}
        while(true){val item=external.pollFirst()?:break;runCatching{item.close()}}
    }
    fun finish(){
        cancelled.set(true);runCatching{control.cancel()};closeResources();control.close()
    }
    companion object{private val cleanup=Executors.newCachedThreadPool{task->Thread(task,"wasmdroid-data-cleanup").apply{isDaemon=true}}}
}
