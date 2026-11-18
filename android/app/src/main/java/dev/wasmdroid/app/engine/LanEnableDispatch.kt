package dev.wasmdroid.app.engine

import java.util.concurrent.atomic.AtomicBoolean
import java.util.concurrent.atomic.AtomicInteger

/** Main reserves before starting a native thread. Cancellation proves no native
 * entry only when it wins this same dispatch CAS. */
internal class LanEnableDispatch {
    private val state=AtomicInteger(RESERVED)
    private val allowedFlag=AtomicBoolean(true)
    val allowed:Boolean get()=allowedFlag.get()
    fun dispatch():Boolean=state.compareAndSet(RESERVED,DISPATCHED)
    fun cancel(){allowedFlag.set(false);state.compareAndSet(RESERVED,CANCELLED)}
    val cancelledBeforeDispatch:Boolean get()=state.get()==CANCELLED
    val reserved:Boolean get()=state.get()==RESERVED
    companion object {private const val RESERVED=0;private const val DISPATCHED=1;private const val CANCELLED=2}
}
