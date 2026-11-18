package dev.wasmdroid.app

import android.os.Bundle
import android.content.Intent
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.lifecycle.ViewModelProvider
import dev.wasmdroid.app.ui.AppModel
import dev.wasmdroid.app.ui.WasmDroidApp

class MainActivity : ComponentActivity() {
    private lateinit var model:AppModel
    private var shortcutIntent:Intent?=null
    @Volatile private var shortcutVisible=false
    override fun attachBaseContext(newBase:android.content.Context) {
        super.attachBaseContext(englishContext(newBase))
    }
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        model = ViewModelProvider(this, ViewModelProvider.AndroidViewModelFactory.getInstance(application))[AppModel::class.java]
        if(savedInstanceState==null)shortcutIntent=intent
        setContent { WasmDroidApp(model) }
    }
    override fun onNewIntent(intent:Intent){super.onNewIntent(intent);setIntent(intent);shortcutIntent=intent;deliverShortcut()}
    override fun onResume(){super.onResume();shortcutVisible=true;model.uiVisible(true);deliverShortcut()}
    override fun onPause(){shortcutVisible=false;if(::model.isInitialized)model.uiVisible(false);super.onPause()}
    override fun onDestroy(){shortcutVisible=false;if(::model.isInitialized)model.uiVisible(false);super.onDestroy()}
    private fun deliverShortcut(){
        if(!shortcutVisible)return
        val requested=shortcutIntent?:return;shortcutIntent=null
        model.handleShortcutIntent(requested){shortcutVisible&&!isFinishing&&!isDestroyed&&!getSystemService(android.app.KeyguardManager::class.java).isDeviceLocked}
    }
}
