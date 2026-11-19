package dev.wasmdroid.app.shortcuts

import android.content.Context
import java.util.UUID

/** Caller supplies one synchronized storage boundary. Tombstones invalidate old
 * widget IDs and captured callbacks; no process-local counter is authority. */
internal class ShortcutSelectionLedger(
    private val read:(String)->String?,private val write:(String,String)->Boolean,
    private val token:()->String={UUID.randomUUID().toString()},
){
    fun current(scope:String):ShortcutSelection{
        require(ShortcutSelection.validScope(scope))
        val saved=read(scope)
        if(saved!=null){
            val parsed=ShortcutSelection.parse(saved)
            check(parsed.scope==scope){"Shortcut selection is unavailable"}
            return parsed
        }
        val initial=ShortcutSelection(scope,1,token(),null)
        check(write(scope,initial.json())){"Shortcut selection could not be saved"}
        return initial
    }
    fun matches(captured:ShortcutSelection):Boolean=runCatching{current(captured.scope)==captured}.getOrDefault(false)
    fun replace(expected:ShortcutSelection,target:ShortcutTarget?):ShortcutSelection{
        check(matches(expected)){"Shortcut selection changed"}
        check(expected.version<Long.MAX_VALUE){"Shortcut selection revision is exhausted"}
        val replacement=ShortcutSelection(expected.scope,expected.version+1,token(),target)
        check(replacement.token!=expected.token){"Shortcut selection token was reused"}
        check(write(expected.scope,replacement.json())){"Shortcut selection could not be saved"}
        return replacement
    }
}

internal object ShortcutSelections{
    private val lock=Any()
    private val blocked=mutableSetOf<String>()
    private fun ledger(context:Context):ShortcutSelectionLedger{
        val prefs=context.applicationContext.getSharedPreferences("scoped-shortcuts",Context.MODE_PRIVATE)
        return ShortcutSelectionLedger({key->
            check(key !in blocked){"Shortcut selection persistence is unavailable"}
            prefs.getString(key,null)
        },{key,value->
            val saved=prefs.edit().putString(key,value).commit()
            // SharedPreferences may update process memory before a failed disk
            // commit. Do not let that unacknowledged snapshot grant actions.
            if(!saved)blocked.add(key)
            saved
        })
    }
    fun current(context:Context,scope:String):ShortcutSelection=synchronized(lock){ledger(context).current(scope)}
    fun matches(context:Context,selection:ShortcutSelection):Boolean=synchronized(lock){
        runCatching{ledger(context).matches(selection)}.getOrDefault(false)
    }
    fun replace(context:Context,expected:ShortcutSelection,target:ShortcutTarget?):ShortcutSelection=synchronized(lock){
        ledger(context).replace(expected,target)
    }
    fun invalidate(context:Context,scope:String)=synchronized(lock){
        val ledger=ledger(context)
        ledger.replace(ledger.current(scope),null)
    }
}
