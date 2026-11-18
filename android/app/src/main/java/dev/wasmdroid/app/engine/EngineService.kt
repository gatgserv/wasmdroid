package dev.wasmdroid.app.engine

import android.app.*
import android.content.Intent
import android.os.*
import dev.wasmdroid.app.MainActivity
import dev.wasmdroid.app.R
import dev.wasmdroid.app.englishContext
import org.json.JSONObject

/** The common coordinator outlives Activity recreation/closure. Android may
 * still terminate processes; persisted state and inherited ownership handle it. */
class EngineService : Service() {
    override fun attachBaseContext(newBase:android.content.Context) {
        super.attachBaseContext(englishContext(newBase))
    }
    companion object {
        const val ACTION_STOP_ALL = "dev.wasmdroid.app.STOP_ALL"
        const val ACTION_RESTORE_BOOT = "dev.wasmdroid.app.RESTORE_BOOT"
        const val ACTION_SHORTCUT_BIND = "dev.wasmdroid.app.SHORTCUT_BIND"
        private const val ACTION_RECOVER_OWNER = "dev.wasmdroid.app.RECOVER_OWNER"
        private const val CHANNEL = "local-services"
        private const val NOTIFICATION = 1
    }
    private val bindingGroups=mutableSetOf<Intent.FilterComparison>()
    private fun markBound(intent:Intent?){
        val first=bindingGroups.isEmpty()
        bindingGroups.add(Intent.FilterComparison(intent?.let(::Intent)?:Intent(this,EngineService::class.java)))
        if(first)supervisor.ownerBound()
    }
    private val main = Handler(Looper.getMainLooper())
    lateinit var supervisor: ServiceSupervisor
        private set
    private var foreground = false
    private var stopWhenIdle = false
    private var restoringBoot = false
    private var restoringRecovery=false
    private var recoveryRequested=false
    private var bootRestored = false
    private var latestStartId=0
    private var destroyed=false
    private var notificationSummary=NotificationSummary.summarize(emptyList(),false)
    inner class LocalBinder : Binder() { val engine: ServiceSupervisor get() = supervisor }
    private val binder = LocalBinder()

    override fun onCreate() {
        super.onCreate()
        getSystemService(NotificationManager::class.java).createNotificationChannel(
            NotificationChannel(CHANNEL, getString(R.string.notification_channel), NotificationManager.IMPORTANCE_LOW))
        supervisor = ServiceSupervisor(this) { activity -> main.post {
            if(destroyed)return@post
            notificationSummary=activity.summary
            // Background reads can update an existing notification, but cannot
            // create a new foreground service on their own.
            if(activity.ownedCount>0||(foreground&&activity.summary.backgroundWork))showForeground()
            if(activity.ownedCount==0&&!restoringBoot&&!restoringRecovery)requestIdleRetirement()
        } }
    }
    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        latestStartId=startId
        supervisor.ownerStarted()
        showForeground()
        if (intent?.action == ACTION_RESTORE_BOOT && !bootRestored && !stopWhenIdle) {
            bootRestored = true
            restoringBoot = true
            supervisor.restoreBootApplications().whenComplete { report, error -> main.post {
                restoringBoot = false
                val failed = error != null || runCatching { JSONObject(report).getInt("failed") > 0 }.getOrDefault(true)
                val cancelled = runCatching { JSONObject(report).optBoolean("cancelled",false) }.getOrDefault(false)
                BootAdmissionReceipt.record(this, when {cancelled->"restore_cancelled";failed->"restore_failed";else->"restore_completed"})
                requestIdleRetirement()
            } }
        } else if(intent?.action==ACTION_RECOVER_OWNER&&!stopWhenIdle){
            restoringRecovery=true
            supervisor.recoverAuthorizedApplications().whenComplete{_,_->main.post{
                restoringRecovery=false;requestIdleRetirement()
            }}
        } else if (intent?.action == ACTION_STOP_ALL) {
            stopWhenIdle = true
            supervisor.stopAll().whenComplete { _, error -> main.post {
                if (error == null) requestIdleRetirement()
            } }
        }
        // Only explicit boot and configured restart policies launch services;
        // sticky recreation and automatic rebind never resurrect applications.
        return START_NOT_STICKY
    }
    override fun onBind(intent: Intent?): IBinder {
        markBound(intent)
        maybeRecover(intent)
        return binder
    }
    private fun maybeRecover(intent:Intent?){
        if(intent?.action==ACTION_SHORTCUT_BIND)return
        if(!recoveryRequested){
            recoveryRequested=true
            // A bound-only owner would be destroyed when Activity unbinds.
            // Establish the actual started foreground lifetime before admitting
            // any authorized recovery guest; OS denial starts no guest.
            try{startForegroundService(Intent(this,EngineService::class.java).setAction(ACTION_RECOVER_OWNER))}
            catch(_:RuntimeException){supervisor.noteRecoveryUnavailable("foreground_start_denied")}
        }
    }
    override fun onRebind(intent:Intent?){markBound(intent);maybeRecover(intent);super.onRebind(intent)}
    override fun onUnbind(intent:Intent?):Boolean {
        val removed=bindingGroups.remove(Intent.FilterComparison(intent?.let(::Intent)?:Intent(this,EngineService::class.java)))
        if(removed&&bindingGroups.isEmpty()){supervisor.ownerUnbound();requestIdleRetirement()}
        return true
    }
    private fun requestIdleRetirement(){
        if(restoringBoot||restoringRecovery||destroyed)return
        val startId=latestStartId
        supervisor.requestIdleRetirement().whenComplete{permit,_->if(permit!=null)main.post{
            if(destroyed)return@post
            supervisor.commitIdleRetirement(permit){stopService->
                if(foreground){stopForeground(STOP_FOREGROUND_REMOVE);foreground=false}
                if(stopService&&startId!=0)stopSelfResult(startId)
            }
        }}
    }
    private fun showForeground() {
        if(destroyed)return
        val open = PendingIntent.getActivity(this, 0, Intent(this, MainActivity::class.java),
            PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE)
        val stop = PendingIntent.getService(this, 1, Intent(this, EngineService::class.java).setAction(ACTION_STOP_ALL),
            PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE)
        val notification = Notification.Builder(this, CHANNEL)
            .setSmallIcon(android.R.drawable.ic_media_play)
            .setContentTitle("WasmDroid")
            .setContentText(notificationText())
            .setContentIntent(open).setOngoing(true)
            .addAction(Notification.Action.Builder(null, getString(R.string.stop_all), stop).build())
            .build()
        startForeground(NOTIFICATION, notification)
        foreground = true
    }
    private fun notificationText():String {
        val summary=notificationSummary
        val parts=mutableListOf<String>()
        if(summary.runningApplications>0)parts.add(resources.getQuantityString(R.plurals.notification_apps_active,summary.runningApplications,summary.runningApplications))
        if(summary.startingApplications>0)parts.add(resources.getQuantityString(R.plurals.notification_apps_starting,summary.startingApplications,summary.startingApplications))
        if(summary.stoppingApplications>0)parts.add(resources.getQuantityString(R.plurals.notification_apps_stopping,summary.stoppingApplications,summary.stoppingApplications))
        if(summary.backgroundWork)parts.add(getString(R.string.notification_background_work))
        return parts.joinToString(" · ").ifEmpty{getString(R.string.notification_preparing)}
    }
    override fun onDestroy() {
        destroyed=true
        supervisor.close()
        super.onDestroy()
    }
}
