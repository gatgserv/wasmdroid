package dev.wasmdroid.app.shortcuts

import android.content.ComponentName
import android.content.Context
import android.content.Intent
import android.content.ServiceConnection
import android.os.Handler
import android.os.IBinder
import android.os.Looper
import dev.wasmdroid.app.engine.EngineService
import dev.wasmdroid.app.engine.ServiceSupervisor
import java.util.concurrent.CompletableFuture

/** Read-only binding cannot implicitly authorize recovery or foreground work. */
internal class ShortcutOwnerConnection(private val context:Context):AutoCloseable{
    val ready=CompletableFuture<ServiceSupervisor>()
    private val main=Handler(Looper.getMainLooper())
    private var bound=false
    private var closed=false
    private val timeout=Runnable{ready.completeExceptionally(IllegalStateException("Shortcut owner is unavailable"));close()}
    private val connection=object:ServiceConnection{
        override fun onServiceConnected(name:ComponentName?,binder:IBinder?){
            val owner=(binder as? EngineService.LocalBinder)?.engine
            if(closed||owner==null){ready.completeExceptionally(IllegalStateException("Shortcut owner is unavailable"));return}
            main.removeCallbacks(timeout);ready.complete(owner)
        }
        override fun onServiceDisconnected(name:ComponentName?){
            ready.completeExceptionally(IllegalStateException("Shortcut owner disconnected"))
        }
        override fun onBindingDied(name:ComponentName?){close()}
        override fun onNullBinding(name:ComponentName?){close()}
    }
    init{
        check(Looper.myLooper()==Looper.getMainLooper())
        try{
            bound=context.bindService(Intent(context,EngineService::class.java).setAction(EngineService.ACTION_SHORTCUT_BIND),connection,Context.BIND_AUTO_CREATE)
            if(!bound)ready.completeExceptionally(IllegalStateException("Shortcut owner is unavailable"))
            else main.postDelayed(timeout,10_000)
        }catch(_:RuntimeException){ready.completeExceptionally(IllegalStateException("Shortcut owner is unavailable"))}
    }
    override fun close(){
        if(Looper.myLooper()!=Looper.getMainLooper()){main.post{close()};return}
        if(closed)return
        closed=true;main.removeCallbacks(timeout)
        ready.completeExceptionally(IllegalStateException("Shortcut owner is unavailable"))
        if(bound){runCatching{context.unbindService(connection)};bound=false}
    }
}
