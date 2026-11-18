package dev.wasmdroid.app.shortcuts

import android.app.PendingIntent
import android.content.Context
import android.content.Intent
import android.net.Uri
import java.security.MessageDigest

internal object ShortcutIntents{
    const val EXTRA_CLICK="dev.wasmdroid.app.shortcut.CLICK"
    const val EXTRA_SELECTION="dev.wasmdroid.app.shortcut.SELECTION"
    const val EXTRA_TARGET="dev.wasmdroid.app.shortcut.TARGET"
    const val EXTRA_STATE_TOKEN="dev.wasmdroid.app.shortcut.STATE_TOKEN"
    const val EXTRA_REVIEW="dev.wasmdroid.app.shortcut.REVIEW"

    private fun digest(value:String)=MessageDigest.getInstance("SHA-256").digest(value.toByteArray(Charsets.UTF_8))
        .joinToString(""){"%02x".format(java.util.Locale.ROOT,it.toInt() and 255)}

    fun actionIntent(context:Context,click:ShortcutClick):Intent=Intent(context,ShortcutActionActivity::class.java)
        .setData(Uri.Builder().scheme("wasmdroid-shortcut").authority("action")
            .appendPath(click.selection.scope).appendPath(click.selection.version.toString())
            .appendPath(click.selection.token).appendPath(click.action.wire).appendPath(digest(click.stateToken)).build())
        .putExtra(EXTRA_CLICK,click.json())

    fun action(context:Context,click:ShortcutClick):PendingIntent=PendingIntent.getActivity(context,0,
        actionIntent(context,click),PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE)

    fun configurationIntent(context:Context,selection:ShortcutSelection):Intent=
        Intent(context,ShortcutConfigurationActivity::class.java)
            .setData(Uri.Builder().scheme("wasmdroid-shortcut").authority("configure")
                .appendPath(selection.scope).appendPath(selection.token).build())
            .putExtra(EXTRA_SELECTION,selection.json())

    fun configuration(context:Context,selection:ShortcutSelection):PendingIntent=PendingIntent.getActivity(context,0,
        configurationIntent(context,selection),PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE)

    fun selectionCurrent(context:Context,intent:Intent):Boolean=runCatching{
        val click=ShortcutClick.parse(requireNotNull(intent.getStringExtra(EXTRA_CLICK)))
        selectionCurrent(context,click.selection)
    }.getOrDefault(false)

    fun selectionCurrent(context:Context,selection:ShortcutSelection):Boolean=ShortcutSelections.matches(context,selection)&&
        (selection.scope=="tile"||ShortcutWidgets.owns(context,selection.scope.removePrefix("widget:").toInt()))
}
