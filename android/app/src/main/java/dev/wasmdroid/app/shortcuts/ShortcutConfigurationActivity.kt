package dev.wasmdroid.app.shortcuts

import android.appwidget.AppWidgetManager
import android.content.Context
import android.content.Intent
import android.os.Bundle
import android.service.quicksettings.TileService
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.selection.selectable
import androidx.compose.foundation.selection.selectableGroup
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.runtime.saveable.LocalSaveableStateRegistry
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.unit.dp
import androidx.lifecycle.ViewModelProvider
import dev.wasmdroid.app.R
import dev.wasmdroid.app.englishContext

class ShortcutConfigurationActivity:ComponentActivity(){
    override fun attachBaseContext(base:Context){super.attachBaseContext(englishContext(base))}
    override fun onCreate(savedInstanceState:Bundle?){
        super.onCreate(savedInstanceState)
        setResult(RESULT_CANCELED)
        val supplied=runCatching{intent.getStringExtra(ShortcutIntents.EXTRA_SELECTION)?.let{ShortcutSelection.parse(it)}}.getOrNull()
        val widgetId=intent.getIntExtra(AppWidgetManager.EXTRA_APPWIDGET_ID,AppWidgetManager.INVALID_APPWIDGET_ID)
        val surface=when{
            widgetId>0&&ShortcutWidgets.owns(this,widgetId)->ShortcutSelection.widget(widgetId)
            supplied?.scope?.startsWith("widget:")==true&&ShortcutWidgets.owns(this,supplied.scope.removePrefix("widget:").toInt())->supplied.scope
            supplied?.scope=="tile"||intent.action==TileService.ACTION_QS_TILE_PREFERENCES->"tile"
            else->{finish();return}
        }
        if(supplied!=null&&supplied.scope!=surface){finish();return}
        val model=ViewModelProvider(this,ViewModelProvider.AndroidViewModelFactory.getInstance(application))[ShortcutConfigurationModel::class.java]
        model.initialize(surface,supplied)
        setContent{
            val state by model.state.collectAsState()
            LaunchedEffect(state.saved){if(state.saved){
                if(surface.startsWith("widget:")){
                    val id=surface.removePrefix("widget:").toInt()
                    ShortcutWidgets.refresh(this@ShortcutConfigurationActivity,intArrayOf(id))
                    setResult(RESULT_OK,Intent().putExtra(AppWidgetManager.EXTRA_APPWIDGET_ID,id))
                }else setResult(RESULT_OK)
                finish()
            }}
            CompositionLocalProvider(LocalSaveableStateRegistry provides null){MaterialTheme{
                Surface(Modifier.fillMaxSize()){
                    Column(Modifier.fillMaxSize().verticalScroll(rememberScrollState()).padding(24.dp),verticalArrangement=Arrangement.spacedBy(12.dp)){
                        Text(stringResource(if(surface=="tile")R.string.shortcuts_tile_configuration else R.string.shortcuts_widget_configuration),style=MaterialTheme.typography.headlineSmall)
                        Text(stringResource(R.string.shortcuts_save_does_not_start))
                        if(surface=="tile")Text(stringResource(R.string.shortcuts_review_hint))
                        Text(stringResource(R.string.shortcuts_choose_application),style=MaterialTheme.typography.titleMedium)
                        if(state.targets.isEmpty()&&!state.busy)Text(stringResource(R.string.shortcuts_no_applications))
                        Column(Modifier.selectableGroup()){
                            state.targets.forEach{descriptor->
                                val epoch=state.epoch
                                val target=descriptor.target
                                val selected=state.chosen?.sameIdentity(target)==true
                                Row(Modifier.fillMaxWidth().heightIn(min=56.dp).selectable(selected,!state.busy,Role.RadioButton){model.choose(epoch,target)},verticalAlignment=Alignment.CenterVertically){
                                    RadioButton(selected,onClick=null,enabled=!state.busy)
                                    Column(Modifier.padding(start=12.dp)){
                                        Text(descriptor.name)
                                        Text(stringResource(ShortcutPresentation.stateText(descriptor.state)),style=MaterialTheme.typography.bodySmall)
                                    }
                                }
                            }
                        }
                        if(state.busy)CircularProgressIndicator()
                        state.problem?.let{Text(stringResource(it),color=MaterialTheme.colorScheme.error)}
                        val epoch=state.epoch
                        Row(horizontalArrangement=Arrangement.spacedBy(8.dp)){
                            OutlinedButton(onClick={model.reload(epoch)},enabled=!state.busy){Text(stringResource(R.string.shortcuts_reload))}
                            if(state.next!=null)OutlinedButton(onClick={model.more(epoch)},enabled=!state.busy){Text(stringResource(R.string.shortcuts_next))}
                        }
                        Row(horizontalArrangement=Arrangement.spacedBy(8.dp)){
                            TextButton(onClick={finish()}){Text(stringResource(R.string.shortcuts_cancel))}
                            val chosen=state.chosen
                            Button(onClick={if(chosen!=null)model.save(epoch,chosen)},enabled=!state.busy&&chosen!=null&&state.targets.any{it.target==chosen}){Text(stringResource(R.string.shortcuts_save))}
                        }
                    }
                }
            }}
        }
    }
}
