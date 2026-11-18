package dev.wasmdroid.app.shortcuts

import android.appwidget.AppWidgetManager
import android.appwidget.AppWidgetProvider
import android.content.Context

class ShortcutWidgetProvider:AppWidgetProvider(){
    override fun onUpdate(context:Context,manager:AppWidgetManager,ids:IntArray){
        val pending=goAsync()
        ShortcutWidgets.refresh(context,ids){pending.finish()}
    }
    override fun onDeleted(context:Context,ids:IntArray){
        for(id in ids)runCatching{ShortcutSelections.invalidate(context,ShortcutSelection.widget(id))}
    }
    override fun onRestored(context:Context,oldIds:IntArray,newIds:IntArray){
        // Restored launcher IDs require explicit selection; old capabilities
        // cannot silently migrate to another widget instance or restored app.
        for(id in oldIds+newIds)runCatching{ShortcutSelections.invalidate(context,ShortcutSelection.widget(id))}
        ShortcutWidgets.refresh(context,newIds)
    }
}
