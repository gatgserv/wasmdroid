package dev.wasmdroid.app.engine

/** One explicit user's On attempt. Cancellation survives UI observer disposal
 * and attaches only to the exact owner reservation, never a guessed epoch. */
internal class LanStartIntent {
    private val lock=Any()
    private var stopped=false
    private var attached=false
    private var callback:(()->Unit)?=null
    val cancelled:Boolean get()=synchronized(lock){stopped}
    fun cancel(){
        val action=synchronized(lock){
            if(stopped)return
            stopped=true;callback.also{callback=null}
        }
        action?.invoke()
    }
    internal fun onCancelled(action:()->Unit):AutoCloseable {
        val notify=synchronized(lock){
            check(!attached){"LAN start intent already owns a reservation"}
            attached=true
            if(stopped)true else{callback=action;false}
        }
        if(notify)action()
        return AutoCloseable{synchronized(lock){if(callback===action)callback=null}}
    }
}
