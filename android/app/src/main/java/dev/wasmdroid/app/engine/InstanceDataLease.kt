package dev.wasmdroid.app.engine

import java.util.concurrent.atomic.AtomicBoolean

/** An await-cancellation-safe administrative reservation. An old lease cannot
 * release a newer reservation for the same instance. */
class InstanceDataLease internal constructor(val record:String,private val release:()->Unit):AutoCloseable {
    private val closed=AtomicBoolean(false)
    override fun close(){if(closed.compareAndSet(false,true))release()}
}
