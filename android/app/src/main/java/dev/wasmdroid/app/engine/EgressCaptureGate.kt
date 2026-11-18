package dev.wasmdroid.app.engine

/** A one-use private observation fence. Invalidation is irreversible and never a
 * socket/provider cleanup receipt. Callback delivery is outside this gate lock. */
internal class EgressCaptureGate(private val admitted:()->Boolean) {
    private val lock=Any()
    @Volatile private var active=true
    private var installed=false
    private var revoke:(()->Unit)?=null
    val current:Boolean get()=active&&admitted()
    fun install(action:()->Unit):Boolean {
        val notify=synchronized(lock){
            if(installed)return false
            installed=true
            if(active){revoke=action;false}else true
        }
        if(notify)runCatching(action)
        if(!current)invalidate()
        return current
    }
    fun invalidate(){
        val callback=synchronized(lock){
            if(!active)return
            active=false
            revoke.also{revoke=null}
        }
        callback?.let{runCatching(it)}
    }
}
