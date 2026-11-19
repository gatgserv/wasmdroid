package dev.wasmdroid.app

import android.Manifest
import android.app.PendingIntent
import android.content.ComponentName
import android.os.Build
import androidx.test.platform.app.InstrumentationRegistry
import dev.wasmdroid.app.shortcuts.*
import org.junit.Assert.*
import org.junit.Test

/** Hermetic selection/admission and installed-component definitions. No guest,
 * launcher mutation, unlock, PendingIntent send or background start is executed. */
class ShortcutSelectionTest{
    private val alpha=ShortcutTarget("application","app-alpha","lineage-alpha")
    private val beta=ShortcutTarget("group","group-beta","birth-beta")
    private class Storage{
        val values=mutableMapOf<String,String>()
        var writes=0
        var fail=false
        private var tokens=0
        fun ledger()=ShortcutSelectionLedger({values[it]},{key,value->
            writes++
            if(fail)false else{values[key]=value;true}
        },{"token-${++tokens}"})
    }
    private fun descriptor(target:ShortcutTarget=alpha,state:String="stopped",token:String="state-1",
        start:Boolean=true,stop:Boolean=false,open:Boolean=false)=ShortcutDescriptor(target,"Chosen app",state,token,start,stop,open)
    private fun selected(storage:Storage,scope:String="widget:1",target:ShortcutTarget=alpha):ShortcutSelection{
        val ledger=storage.ledger()
        return ledger.replace(ledger.current(scope),target)
    }
    private fun refuses(action:()->Unit){
        try{action();fail("Expected a refusal")}
        catch(_:IllegalArgumentException){}
        catch(_:IllegalStateException){}
    }

    @Test fun widgetsAndTileHaveIndependentDurableSelections(){
        val storage=Storage();val ledger=storage.ledger()
        val first=ledger.replace(ledger.current("widget:1"),alpha)
        val second=ledger.replace(ledger.current("widget:2"),beta)
        val tile=ledger.current("tile")
        assertNull(tile.target)
        ledger.replace(tile,beta)
        val reopened=storage.ledger()
        assertEquals(first,reopened.current("widget:1"))
        assertEquals(second,reopened.current("widget:2"))
        assertEquals(beta,reopened.current("tile").target)
    }

    @Test fun secondConfigurationWindowCannotOverwriteNewerSelection(){
        val storage=Storage();val ledger=storage.ledger()
        val original=ledger.current("widget:1")
        val updated=ledger.replace(original,alpha)
        val writes=storage.writes
        refuses{ledger.replace(original,beta)}
        assertEquals(writes,storage.writes)
        assertEquals(updated,ledger.current("widget:1"))
    }

    @Test fun deletingAndReusingWidgetIdNeverRevivesAnOldClick(){
        val storage=Storage();val ledger=storage.ledger()
        val original=ledger.replace(ledger.current("widget:1"),alpha)
        val tombstone=ledger.replace(original,null)
        assertFalse(ledger.matches(original))
        val recreated=storage.ledger().replace(tombstone,alpha)
        assertTrue(recreated.version>original.version)
        assertNotEquals(original.token,recreated.token)
        assertFalse(storage.ledger().matches(original))
    }

    @Test fun failedPersistenceCannotAcknowledgeAReplacement(){
        val storage=Storage();val ledger=storage.ledger()
        val original=selected(storage)
        storage.fail=true
        refuses{ledger.replace(original,beta)}
        storage.fail=false
        assertEquals(original,storage.ledger().current("widget:1"))
    }

    @Test fun corruptOrCrossScopeSavedStateFailsClosed(){
        val storage=Storage();val ledger=storage.ledger()
        storage.values["widget:1"]="not a selection"
        assertFalse(ledger.matches(ShortcutSelection("widget:1",1,"capture",alpha)))
        val tile=ledger.current("tile")
        storage.values["widget:1"]=tile.json()
        refuses{ledger.current("widget:1")}
        assertNull(ledger.current("tile").target)
    }

    @Test fun exhaustedRevisionDoesNotWrapOrPublishAnotherTarget(){
        val storage=Storage();val ledger=storage.ledger()
        val exhausted=ShortcutSelection("widget:1",Long.MAX_VALUE,"last-token",alpha)
        storage.values["widget:1"]=exhausted.json()
        refuses{ledger.replace(exhausted,beta)}
        assertEquals(0,storage.writes)
        assertEquals(exhausted,ledger.current("widget:1"))
    }

    @Test fun invalidScopeAndUnknownGlobalActionCannotBeCaptured(){
        for(scope in listOf("widget:0","widget:-1","widget:01","widget:2147483648","tile:1","all"))
            refuses{ShortcutSelection(scope,1,"token",alpha)}
        refuses{ShortcutAction.parse("stop_all")}
        refuses{ShortcutClick(ShortcutSelection("tile",1,"token",null),ShortcutAction.Start,"state")}
    }

    @Test fun selectionAndActionRoundTripPreserveExactCapturedAuthority(){
        val original=selected(Storage())
        val click=ShortcutClick(original,ShortcutAction.Stop,"generation-version-snapshot")
        assertEquals(click,ShortcutClick.parse(click.json()))
        assertEquals(original,ShortcutSelection.parse(original.json()))
        assertFalse(click.json().contains("environment"))
        assertFalse(click.json().contains("password"))
    }

    @Test fun queuedOldActionRechecksSelectionAtActualAdmission(){
        val storage=Storage();val ledger=storage.ledger()
        val original=selected(storage)
        val click=ShortcutClick(original,ShortcutAction.Start,"state-1")
        val guard=ShortcutAdmission(click,{ledger.matches(original)},{true},{true})
        assertTrue(guard.current())
        // Fake owner preflight has not reserved anything yet. A later Save
        // completes first; invoking the exact retained guard must refuse it.
        val queuedAdmission=guard::current
        ledger.replace(original,beta)
        var dispatched=0
        if(queuedAdmission())dispatched++
        assertEquals(0,dispatched)
        assertEquals(beta,ledger.current("widget:1").target)
    }

    @Test fun lockedPausedOrUnavailableAuthorityCannotAdmitQueuedWork(){
        val selection=selected(Storage())
        val click=ShortcutClick(selection,ShortcutAction.Start,"state-1")
        var visible=true;var unlocked=true;var current=true
        val guard=ShortcutAdmission(click,{current},{visible},{unlocked})
        assertTrue(guard.current())
        unlocked=false;assertFalse(guard.current())
        unlocked=true;visible=false;assertFalse(guard.current())
        visible=true;current=false;assertFalse(guard.current())
        val failed=ShortcutAdmission(click,{error("private backend failure")},{true},{true})
        assertFalse(failed.current())
    }

    @Test fun reusedNativeIdCannotPassAStaleSelectionIdentity(){
        val selection=selected(Storage())
        val click=ShortcutClick(selection,ShortcutAction.Start,"state-1")
        val guard=ShortcutAdmission(click,{true},{true},{true})
        assertTrue(guard.resolves(descriptor()))
        val replaced=alpha.copy(identityToken="different-incarnation")
        assertFalse(guard.resolves(descriptor(target=replaced)))
        assertFalse(guard.resolves(descriptor(target=beta)))
    }

    @Test fun sameApplicationRetryKeepsLineageButRequiresFreshGenerationToken(){
        val selection=selected(Storage())
        val oldClick=ShortcutClick(selection,ShortcutAction.Stop,"old-running-generation")
        val replacement=alpha.copy(id="app-new-native-id")
        val actual=descriptor(replacement,"running","new-running-generation",start=false,stop=true)
        assertFalse(ShortcutAdmission(oldClick,{true},{true},{true}).resolves(actual))
        val freshClick=oldClick.copy(stateToken=actual.stateToken)
        assertTrue(ShortcutAdmission(freshClick,{true},{true},{true}).resolves(actual))
        val reusedGroup=beta.copy(id="different-group-id")
        assertFalse(beta.sameIdentity(reusedGroup))
    }

    @Test fun partialTileOpensReviewAndNeverGuessesAStartOrStop(){
        assertEquals(ShortcutAction.Start,ShortcutPresentation.tileAction(descriptor()))
        assertEquals(ShortcutAction.Stop,ShortcutPresentation.tileAction(descriptor(state="running",start=false,stop=true)))
        assertEquals(ShortcutAction.Review,ShortcutPresentation.tileAction(descriptor(state="partial",start=true,stop=true)))
        for(state in listOf("starting","stopping","unavailable"))assertNull(ShortcutPresentation.tileAction(descriptor(state=state)))
        assertNull(ShortcutPresentation.tileAction(descriptor(start=false)))
        assertNull(ShortcutPresentation.tileAction(descriptor(state="running",stop=false)))
    }

    @Test fun pendingIntentIdentitySeparatesSelectionActionAndGenerationSnapshots(){
        val context=InstrumentationRegistry.getInstrumentation().targetContext
        val selection=selected(Storage())
        val click=ShortcutClick(selection,ShortcutAction.Start,"state-1")
        val intents=listOf(click,click.copy(action=ShortcutAction.Stop),click.copy(stateToken="state-2"),
            click.copy(selection=selection.copy(version=selection.version+1,token="new-selection-token")))
            .map{ShortcutIntents.actionIntent(context,it)}
        assertEquals(4,intents.map{it.data}.toSet().size)
        for(intent in intents){
            assertEquals(ComponentName(context,ShortcutActionActivity::class.java),intent.component)
            assertEquals(click.selection.scope,ShortcutClick.parse(requireNotNull(intent.getStringExtra(ShortcutIntents.EXTRA_CLICK))).selection.scope)
        }
        val pending=intents.map{PendingIntent.getActivity(context,0,it,PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE)}
        try{
            assertEquals(4,pending.toSet().size)
            if(Build.VERSION.SDK_INT>=31)pending.forEach{assertTrue(it.isImmutable)}
        }finally{pending.forEach{it.cancel()}}
    }

    @Suppress("DEPRECATION")
    @Test fun installedShortcutComponentsExposeOnlyTheIntendedSystemEntryPoints(){
        val context=InstrumentationRegistry.getInstrumentation().targetContext
        val manager=context.packageManager
        val action=manager.getActivityInfo(ComponentName(context,ShortcutActionActivity::class.java),0)
        assertFalse(action.exported)
        val widget=manager.getReceiverInfo(ComponentName(context,ShortcutWidgetProvider::class.java),0)
        assertFalse(widget.exported)
        val configuration=manager.getActivityInfo(ComponentName(context,ShortcutConfigurationActivity::class.java),0)
        assertTrue(configuration.exported)
        val tile=manager.getServiceInfo(ComponentName(context,ShortcutTileService::class.java),0)
        assertTrue(tile.exported)
        assertEquals(Manifest.permission.BIND_QUICK_SETTINGS_TILE,tile.permission)
    }
}
