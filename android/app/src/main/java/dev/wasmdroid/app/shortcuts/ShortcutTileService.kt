package dev.wasmdroid.app.shortcuts

import android.annotation.SuppressLint
import android.app.PendingIntent
import android.content.Intent
import android.graphics.drawable.Icon
import android.os.Build
import android.os.Handler
import android.os.Looper
import android.service.quicksettings.Tile
import android.service.quicksettings.TileService
import dev.wasmdroid.app.R
import dev.wasmdroid.app.englishContext

class ShortcutTileService:TileService(){
    private val main=Handler(Looper.getMainLooper())
    private var serial=0L
    private var listening=false
    private var selection:ShortcutSelection?=null
    private var descriptor:ShortcutDescriptor?=null
    private var connection:ShortcutOwnerConnection?=null

    override fun onStartListening(){
        super.onStartListening()
        listening=true
        val epoch=++serial
        connection?.close();connection=null;descriptor=null
        selection=runCatching{ShortcutSelections.current(this,"tile")}.getOrNull()
        render()
        val captured=selection?:return
        if(captured.target==null)return
        val bound=ShortcutOwnerConnection(this);connection=bound
        bound.ready.whenComplete{owner,failure->
            if(failure!=null){main.post{if(epoch==serial){bound.close();connection=null;render()}};return@whenComplete}
            owner.shortcutResolve(captured.target.json()).whenComplete{json,error->main.post{
                if(epoch!=serial||!listening)return@post
                bound.close();connection=null
                if(error==null&&ShortcutSelections.matches(this,captured)){
                    descriptor=runCatching{ShortcutDescriptor.parse(json)}.getOrNull()?.takeIf{captured.target.sameIdentity(it.target)}
                }
                render()
            }}
        }
    }
    override fun onStopListening(){listening=false;serial++;connection?.close();connection=null;super.onStopListening()}
    override fun onTileRemoved(){
        runCatching{ShortcutSelections.invalidate(this,"tile")}
        super.onTileRemoved()
    }
    private fun render(){
        val tile=qsTile?:return
        val english=englishContext(this)
        val chosen=selection
        val current=descriptor
        tile.icon=Icon.createWithResource(this,android.R.drawable.ic_media_play)
        tile.label=current?.name?:english.getString(R.string.shortcuts_app_name)
        tile.state=when{
            chosen==null->Tile.STATE_UNAVAILABLE
            chosen.target==null->Tile.STATE_INACTIVE
            current==null||current.state=="unavailable"||current.state=="starting"||current.state=="stopping"->Tile.STATE_UNAVAILABLE
            current.state=="running"->Tile.STATE_ACTIVE
            else->Tile.STATE_INACTIVE
        }
        val status=english.getString(when{
            chosen?.target==null->R.string.shortcuts_choose_application
            isLocked->R.string.shortcuts_unlock_required
            else->ShortcutPresentation.stateText(current?.state?:"unavailable")
        })
        if(Build.VERSION.SDK_INT>=29)tile.subtitle=status
        tile.contentDescription="${tile.label}. $status"
        tile.updateTile()
    }
    override fun onClick(){
        super.onClick()
        val captured=selection?:return
        val current=descriptor
        val intent=if(captured.target==null)ShortcutIntents.configurationIntent(this,captured)
            else{
                val action=current?.let{ShortcutPresentation.tileAction(it)}?:return
                ShortcutIntents.actionIntent(this,ShortcutClick(captured,action,current.stateToken))
            }
        val launch=Runnable{
            if(!isLocked&&ShortcutIntents.selectionCurrent(this,captured))launch(intent)
            else render()
        }
        if(isLocked)unlockAndRun(launch)else launch.run()
    }
    private fun launch(intent:Intent){
        runCatching{
            intent.addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
            if(Build.VERSION.SDK_INT>=Build.VERSION_CODES.UPSIDE_DOWN_CAKE){
                val pending=PendingIntent.getActivity(this,0,intent,PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE)
                startActivityAndCollapse(pending)
            }else launchOnLegacyAndroid(intent)
        }.onFailure{render()}
    }

    // Android 13 and earlier require the Intent overload to collapse Quick Settings.
    @SuppressLint("StartActivityAndCollapseDeprecated")
    @Suppress("DEPRECATION")
    private fun launchOnLegacyAndroid(intent: Intent) {
        startActivityAndCollapse(intent)
    }

    override fun onDestroy(){connection?.close();super.onDestroy()}
}
