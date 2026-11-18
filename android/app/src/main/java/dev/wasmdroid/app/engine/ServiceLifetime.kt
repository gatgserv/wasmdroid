package dev.wasmdroid.app.engine

/** Main/actor lifetime handshake. A zero notification is only a hint. The
 * actor reserves retirement while idle; main commits that exact permit. While
 * unbound retirement is reserved, every later work admission is fenced. New
 * binding/start invalidates the permit without permanently closing a bound UI. */
class ServiceLifetime {
    class IdlePermit internal constructor(internal val revision: Long, val stopService: Boolean)
    private val lock=Any()
    private var revision=0L
    private var bound=false
    private var destroyed=false
    private var owned=0
    private var background=0
    private var retiring:IdlePermit?=null

    fun bound(){synchronized(lock){if(!destroyed){bound=true;retiring=null;revision++}}}
    fun unbound(){synchronized(lock){bound=false;retiring=null;revision++}}
    fun started(){synchronized(lock){if(!destroyed){retiring=null;revision++}}}
    fun destroy(){synchronized(lock){destroyed=true;retiring=null;revision++}}
    fun admits():Boolean=synchronized(lock){!destroyed&&retiring==null}
    fun owned(count:Int){synchronized(lock){
        require(count>=0)
        if(owned!=count){owned=count;retiring=null;revision++}
    }}

    /** IO admission and idle reservation share the same monitor. A native read
     * or definition write cannot begin in a check-then-increment race. */
    fun beginBackground(){synchronized(lock){check(!destroyed&&retiring==null){"Supervisor is retiring"};background++;revision++}}
    fun endBackground(){synchronized(lock){check(background>0);background--;revision++}}
    fun backgroundCount():Int=synchronized(lock){background}

    /** actorIdle includes all controller/admin/package/catalog reservations.
     * Actor guest mutations are serialized with this request; concurrent IO and
     * main lifecycle changes are checked under the monitor as well. */
    fun reserveIdle(actorIdle:Boolean):IdlePermit?=synchronized(lock){
        if(destroyed||!actorIdle||owned!=0||background!=0)return@synchronized null
        retiring?.let{return@synchronized it}
        IdlePermit(revision,!bound).also{if(it.stopService)retiring=it}
    }

    /** Main only. StopSelfResult(startId) is performed inside this exact permit
     * commit; the framework separately protects later accepted start IDs and
     * active binding references. No permanent supervisor close happens here. */
    fun commit(permit:IdlePermit,action:(Boolean)->Unit):Boolean=synchronized(lock){
        if(destroyed||revision!=permit.revision||owned!=0||background!=0||
            (permit.stopService&&(bound||retiring!==permit)))return@synchronized false
        action(permit.stopService)
        true
    }
}
