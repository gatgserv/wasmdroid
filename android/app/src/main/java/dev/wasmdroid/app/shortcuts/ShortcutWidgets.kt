package dev.wasmdroid.app.shortcuts

import android.appwidget.AppWidgetManager
import android.content.ComponentName
import android.content.Context
import android.os.Handler
import android.os.Looper
import android.service.quicksettings.TileService
import android.view.View
import android.widget.RemoteViews
import dev.wasmdroid.app.R
import dev.wasmdroid.app.englishContext
import java.util.concurrent.atomic.AtomicLong

internal object ShortcutWidgets{
    private val main=Handler(Looper.getMainLooper())
    private val serial=AtomicLong()
    private val requests=mutableMapOf<Int,Long>()
    private var refreshPending=false

    /** Call after actual app-state changes, not after read/query lifetime events. */
    fun refreshAll(context:Context){
        val app=context.applicationContext
        main.post{
            if(refreshPending)return@post
            refreshPending=true
            main.postDelayed({
                refreshPending=false
                val ids=AppWidgetManager.getInstance(app).getAppWidgetIds(ComponentName(app,ShortcutWidgetProvider::class.java))
                refresh(app,ids)
                runCatching{TileService.requestListeningState(app,ComponentName(app,ShortcutTileService::class.java))}
            },150)
        }
    }

    fun refresh(context:Context,ids:IntArray,finished:()->Unit={}){
        if(Looper.myLooper()!=Looper.getMainLooper()){main.post{refresh(context,ids,finished)};return}
        val app=context.applicationContext
        val selected=ids.distinct().mapNotNull{id->
            runCatching{
                check(owns(app,id))
                val selection=ShortcutSelections.current(app,ShortcutSelection.widget(id))
                val revision=serial.incrementAndGet();requests[id]=revision
                render(app,id,selection,null)
                Triple(id,selection,revision)
            }.getOrNull()
        }
        if(selected.isEmpty()){finished();return}
        val configured=selected.filter{it.second.target!=null}
        if(configured.isEmpty()){finished();return}
        val connection=ShortcutOwnerConnection(app)
        var left=configured.size
        var completed=false
        val timeout=Runnable{if(!completed){completed=true;connection.close();finished()}}
        main.postDelayed(timeout,8_000)
        fun done(){
            left--
            if(left==0&&!completed){completed=true;main.removeCallbacks(timeout);connection.close();finished()}
        }
        connection.ready.whenComplete{owner,failure->main.post ready@{
            if(completed)return@ready
            if(failure!=null){repeat(configured.size){done()};return@ready}
            for((id,selection,revision)in configured){
                owner.shortcutResolve(requireNotNull(selection.target).json()).whenComplete{json,error->main.post update@{
                    if(completed)return@update
                    if(error==null&&requests[id]==revision&&ShortcutSelections.matches(app,selection)&&owns(app,id)){
                        runCatching{ShortcutDescriptor.parse(json)}.getOrNull()?.takeIf{
                            requireNotNull(selection.target).sameIdentity(it.target)
                        }?.let{render(app,id,selection,it)}
                    }
                    done()
                }}
            }
        }}
    }

    fun owns(context:Context,id:Int):Boolean=id>0&&AppWidgetManager.getInstance(context).getAppWidgetInfo(id)?.provider==
        ComponentName(context,ShortcutWidgetProvider::class.java)

    private fun render(context:Context,id:Int,selection:ShortcutSelection,descriptor:ShortcutDescriptor?){
        val english=englishContext(context)
        val views=RemoteViews(context.packageName,R.layout.shortcuts_widget)
        views.setTextViewText(R.id.shortcuts_name,descriptor?.name?:english.getString(R.string.shortcuts_app_name))
        views.setTextViewText(R.id.shortcuts_state,english.getString(if(selection.target==null)R.string.shortcuts_choose_application
            else ShortcutPresentation.stateText(descriptor?.state?:"unavailable")))
        views.setTextViewText(R.id.shortcuts_start,english.getString(R.string.shortcuts_start))
        views.setTextViewText(R.id.shortcuts_change,english.getString(if(selection.target==null)R.string.shortcuts_choose_application else R.string.shortcuts_change_application))
        views.setOnClickPendingIntent(R.id.shortcuts_change,ShortcutIntents.configuration(context,selection))
        for((view,action)in listOf(R.id.shortcuts_start to ShortcutAction.Start,R.id.shortcuts_stop to ShortcutAction.Stop,R.id.shortcuts_open to ShortcutAction.Open)){
            val enabled=descriptor?.allows(action)==true
            views.setBoolean(view,"setEnabled",enabled)
            views.setViewVisibility(view,if(selection.target==null)View.GONE else View.VISIBLE)
            if(enabled)views.setOnClickPendingIntent(view,ShortcutIntents.action(context,ShortcutClick(selection,action,descriptor!!.stateToken)))
        }
        AppWidgetManager.getInstance(context).updateAppWidget(id,views)
    }
}

internal object ShortcutPresentation{
    fun stateText(state:String):Int=when(state){
        "stopped"->R.string.shortcuts_stopped
        "starting"->R.string.shortcuts_starting
        "running"->R.string.shortcuts_running
        "partial"->R.string.shortcuts_partial
        "stopping"->R.string.shortcuts_stopping
        else->R.string.shortcuts_unavailable
    }
    fun tileAction(descriptor:ShortcutDescriptor):ShortcutAction?=when(descriptor.state){
        "stopped"->ShortcutAction.Start.takeIf{descriptor.canStart}
        "running"->ShortcutAction.Stop.takeIf{descriptor.canStop}
        "partial"->ShortcutAction.Review
        else->null
    }
}
