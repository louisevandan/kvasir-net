package ai.banya.linkcpp.wallet

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.Service
import android.content.Context
import android.content.Intent
import android.os.Build
import android.os.IBinder
import android.provider.Settings
import ai.banya.linkcpp.core.SharedSpec
import ai.banya.linkcpp.core.StakingService
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.cancel
import kotlinx.coroutines.delay
import kotlinx.coroutines.isActive
import kotlinx.coroutines.launch

/**
 * Foreground service that keeps the node agent + spawned worker alive when the
 * screen is off. Unlike iOS (which only gets opportunistic background windows),
 * an Android foreground service runs indefinitely with a persistent notification
 * and can accept inbound connections — so the phone is a real on-demand node.
 */
class NodeService : Service() {
    companion object {
        private const val CHANNEL = "kvasir-node"
        private const val NOTIF_ID = 4201
        // Liveness heartbeat cadence. The settlement service marks a node offline
        // after 300s without a report, so 120s keeps it comfortably online while
        // the worker runs, regardless of which screen (if any) is on top.
        private const val HEARTBEAT_SEC = 120L
        @Volatile var agent: NodeAgentServer? = null; private set

        fun start(ctx: Context, owner: String) {
            val i = Intent(ctx, NodeService::class.java).putExtra("owner", owner)
            if (Build.VERSION.SDK_INT >= 26) ctx.startForegroundService(i) else ctx.startService(i)
        }
        fun stop(ctx: Context) { ctx.stopService(Intent(ctx, NodeService::class.java)) }
    }

    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.IO)
    @Volatile private var beating = false

    override fun onBind(intent: Intent?): IBinder? = null

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        val owner = intent?.getStringExtra("owner") ?: ""
        startForeground(NOTIF_ID, notification())
        if (agent == null) {
            agent = NodeAgentServer(applicationContext).also { it.start(owner) }
        }
        // Heartbeat must live with the service, not a screen. Previously it ran in
        // the Node-settings composable's LaunchedEffect, so the node flipped to
        // offline whenever the user left that screen even though the worker (this
        // foreground service) kept running. Beat from here so liveness tracks the
        // actual worker lifetime.
        if (owner.isNotBlank()) startHeartbeat(owner)
        return START_STICKY
    }

    private fun startHeartbeat(owner: String) {
        if (beating) return
        beating = true
        val nodeId = "android-" +
            (Settings.Secure.getString(contentResolver, Settings.Secure.ANDROID_ID) ?: "dev").take(8)
        val stakingUrl = runCatching {
            getSharedPreferences("linkcpp_cfg", Context.MODE_PRIVATE).getString("stakingUrl", null)
                ?: SharedSpec.loadConstants().stakingServiceUrl
        }.getOrNull().orEmpty()
        if (stakingUrl.isBlank()) return
        val svc = StakingService(stakingUrl)
        scope.launch {
            // Ensure the node exists so heartbeat isn't a 404 on a fresh worker.
            // register() merges server-side (reward fields stay trusted-only), so
            // re-asserting os/deviceKind here never downgrades a benchmarked node.
            runCatching { svc.registerNode(nodeId, owner, os = "android", deviceKind = "phone") }
            while (isActive) {
                runCatching { svc.heartbeat(nodeId) }
                delay(HEARTBEAT_SEC * 1000)
            }
        }
    }

    override fun onDestroy() {
        beating = false
        scope.cancel()
        agent?.stop(); agent = null
        super.onDestroy()
    }

    private fun notification(): Notification {
        if (Build.VERSION.SDK_INT >= 26) {
            val nm = getSystemService(NotificationManager::class.java)
            if (nm.getNotificationChannel(CHANNEL) == null) {
                nm.createNotificationChannel(NotificationChannel(CHANNEL, "Kvasir node",
                    NotificationManager.IMPORTANCE_LOW).apply { setShowBadge(false) })
            }
        }
        val builder = if (Build.VERSION.SDK_INT >= 26) Notification.Builder(this, CHANNEL) else @Suppress("DEPRECATION") Notification.Builder(this)
        return builder
            .setContentTitle("Kvasir 노드 실행 중")
            .setContentText("이 기기가 분산 추론에 참여하고 있습니다")
            .setSmallIcon(android.R.drawable.ic_menu_share)
            .setOngoing(true)
            .build()
    }
}
