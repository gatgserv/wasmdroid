package dev.wasmdroid.app.ui

import android.annotation.SuppressLint
import android.content.Intent
import android.os.Handler
import android.os.Looper
import android.view.ViewGroup
import android.webkit.*
import androidx.activity.compose.BackHandler
import androidx.compose.foundation.layout.*
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.viewinterop.AndroidView
import androidx.compose.ui.unit.dp
import androidx.core.net.toUri
import androidx.webkit.WebViewFeature
import dev.wasmdroid.app.R
import dev.wasmdroid.app.engine.BrowserProfileLease
import dev.wasmdroid.app.engine.BrowserProfiles
import dev.wasmdroid.app.engine.BrowserSessionLease
import kotlinx.coroutines.CancellationException
import org.json.JSONObject
import java.io.ByteArrayInputStream

data class WebTarget(val id:String,val name:String,val url:String,val generation:String,val instancePort:Int,val path:String){
    fun sameEndpoint(other:WebTarget)=id==other.id&&generation==other.generation&&instancePort==other.instancePort&&path==other.path&&url==other.url
}
fun webTarget(json:String,state:AppState):WebTarget? = runCatching {
    val row=JSONObject(json);val id=row.getString("id");val spec=row.getJSONObject("spec")
    val labels=spec.optJSONObject("labels")?:return null
    val port=labels.optString("dev.wasmdroid.web.port").toIntOrNull()?:return null
    val path=labels.optString("dev.wasmdroid.web.path","/")
    if(!path.startsWith('/')||path.startsWith("//")||path.contains('\\')||path.any{it.isISOControl()}) return null
    val endpoint=JSONObject(state.browserEndpoints[id]?:return null)
    if(endpoint.getString("id")!=id)return null
    val generation=endpoint.getString("generation")
    val mappings=endpoint.getJSONArray("ports")
    val binding=(0 until mappings.length()).map{mappings.getJSONObject(it)}.firstOrNull{it.getInt("instance_port")==port&&it.getString("protocol")=="tcp"}?:return null
    WebTarget(id,labels.optString("dev.wasmdroid.ui.name",spec.getString("command")),"http://127.0.0.1:${binding.getInt("host_port")}$path",generation,port,path)
}.getOrNull()

@OptIn(ExperimentalMaterial3Api::class)
@Composable fun LocalWebScreen(target:WebTarget,model:AppModel,back:()->Unit) {
    val context=LocalContext.current
    BackHandler(onBack=back)
    val main=remember{Handler(Looper.getMainLooper())}
    val profiles=remember{BrowserProfiles.get(context)}
    var view by remember(target.id,target.generation) {mutableStateOf<WebView?>(null)}
    var profileLease by remember(target.id,target.generation){mutableStateOf<BrowserProfileLease?>(null)}
    var session by remember(target.id,target.generation){mutableStateOf<BrowserSessionLease?>(null)}
    val isolated = remember { WebViewFeature.isFeatureSupported(WebViewFeature.MULTI_PROFILE) }
    var failed by remember(target.id,target.generation) { mutableStateOf(false) }
    fun release() {
        val previous=view
        if(previous!=null){
            runCatching{previous.stopLoading();previous.loadUrl("about:blank");previous.clearHistory()}
            val destroyed=runCatching{(previous.parent as? ViewGroup)?.removeView(previous);previous.removeAllViews();previous.destroy()}.isSuccess
            if(!destroyed){failed=true;return}
            view=null
        }
        profileLease?.close();profileLease=null
        session?.close();session=null
    }
    DisposableEffect(target.id,target.generation) {onDispose {release()}}
    LaunchedEffect(target.id,target.generation,target.url){
        var admitted:BrowserSessionLease?=null
        try{
            val acquired=model.browserSession(target);admitted=acquired
            check(acquired.valid&&acquired.url==target.url){"Browser session changed"}
            session=acquired
            acquired.onRevoked{main.post{release();failed=true}}
            check(acquired.valid){"Browser session ended"}
            // External-browser admission still uses the current owner lease
            // when this provider cannot host an isolated embedded profile.
            if(!isolated)return@LaunchedEffect
            val retained=profiles.acquire(acquired);profileLease=retained
            val browser=WebView(context)
            view=browser
            // Profile attachment remains the first API after construction.
            profiles.attach(browser,retained)
            configureGuestWebSettings(browser.settings)
            browser.webChromeClient=object:WebChromeClient(){
                override fun onPermissionRequest(request:PermissionRequest){request.deny()}
                override fun onGeolocationPermissionsShowPrompt(origin:String,callback:GeolocationPermissions.Callback){callback.invoke(origin,false,false)}
            }
            browser.webViewClient=object:WebViewClient(){
                override fun shouldOverrideUrlLoading(view:WebView?,request:WebResourceRequest)=!acquired.permitsOrdinaryRequest(request.url.toString())
                override fun shouldInterceptRequest(view:WebView?,request:WebResourceRequest):WebResourceResponse? {
                    val uri=request.url
                    if(acquired.valid&&(acquired.permitsOrdinaryRequest(uri.toString())||(!request.isForMainFrame&&uri.scheme in listOf("data","blob","about"))))return null
                    return WebResourceResponse("text/plain","UTF-8",ByteArrayInputStream(ByteArray(0)))
                }
                override fun onRenderProcessGone(view:WebView?,detail:RenderProcessGoneDetail):Boolean {
                    if(view===browser){release();failed=true}
                    return true
                }
            }
            check(acquired.valid){"Browser session ended before navigation"}
            browser.loadUrl(acquired.url)
        }catch(failure:Exception){
            release();admitted?.close()
            if(failure is CancellationException)throw failure
            failed=true
        }
    }
    Scaffold(topBar={TopAppBar(title={Text(target.name)},navigationIcon={TextButton(onClick=back){Text(stringResource(R.string.back))}},actions={
        TextButton(onClick={runCatching { val current=session?:error("Browser session is unavailable");check(current.valid);context.startActivity(Intent(Intent.ACTION_VIEW,current.url.toUri())) }.onFailure { release();failed=true }}){Text(stringResource(R.string.browser))}
    })}){padding->
        if (!isolated || failed) {
            Column(Modifier.fillMaxSize().padding(padding).padding(24.dp), verticalArrangement=Arrangement.spacedBy(16.dp)) {
                Text(stringResource(if(!isolated) R.string.webview_isolation_required else R.string.webview_failed))
                Text(stringResource(R.string.browser_privacy_notice),style=MaterialTheme.typography.bodyMedium)
            }
            return@Scaffold
        }
        val browser=view?:return@Scaffold
        key(target.id,target.generation){AndroidView(modifier=Modifier.fillMaxSize().padding(padding),factory={browser})}
    }
}

// Guest web apps need scripts. The view uses an isolated profile, denies host
// permissions, and registers no JavaScript bridge to the Android app.
@SuppressLint("SetJavaScriptEnabled")
private fun configureGuestWebSettings(settings: WebSettings) {
    settings.apply {
        javaScriptEnabled = true
        domStorageEnabled = true
        allowFileAccess = false
        allowContentAccess = false
        setGeolocationEnabled(false)
        javaScriptCanOpenWindowsAutomatically = false
        setSupportMultipleWindows(false)
        mixedContentMode = WebSettings.MIXED_CONTENT_NEVER_ALLOW
    }
}
