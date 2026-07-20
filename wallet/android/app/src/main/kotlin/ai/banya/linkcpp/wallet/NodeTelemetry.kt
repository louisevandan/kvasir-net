package ai.banya.linkcpp.wallet

import android.app.ActivityManager
import android.content.Context
import android.content.Intent
import android.content.IntentFilter
import android.os.BatteryManager
import java.io.File

data class DeviceStats(
    val ramTotalMb: Long = 0,
    val ramUsedMb: Long = 0,
    val ramAvailMb: Long = 0,
    val cpuLoad: Float = 0f,      // 0..1, aggregate
    val batteryTempC: Float = 0f, // celsius (thermal proxy)
    val charging: Boolean = false,
)

/** Best-effort device telemetry available to an unprivileged app. */
object NodeTelemetry {
    private var lastIdle = 0L
    private var lastTotal = 0L

    fun read(ctx: Context): DeviceStats {
        val am = ctx.getSystemService(Context.ACTIVITY_SERVICE) as ActivityManager
        val mi = ActivityManager.MemoryInfo()
        am.getMemoryInfo(mi)
        val total = mi.totalMem / (1024 * 1024)
        val avail = mi.availMem / (1024 * 1024)

        val bi = ctx.registerReceiver(null, IntentFilter(Intent.ACTION_BATTERY_CHANGED))
        val temp = (bi?.getIntExtra(BatteryManager.EXTRA_TEMPERATURE, 0) ?: 0) / 10f
        val status = bi?.getIntExtra(BatteryManager.EXTRA_STATUS, -1) ?: -1
        val charging = status == BatteryManager.BATTERY_STATUS_CHARGING ||
            status == BatteryManager.BATTERY_STATUS_FULL

        return DeviceStats(
            ramTotalMb = total, ramUsedMb = total - avail, ramAvailMb = avail,
            cpuLoad = cpuLoad(), batteryTempC = temp, charging = charging,
        )
    }

    private fun cpuLoad(): Float = try {
        val line = File("/proc/stat").bufferedReader().use { it.readLine() } // "cpu u n s idle iowait irq ..."
        val v = line.trim().split(Regex("\\s+")).drop(1).map { it.toLong() }
        val idle = v[3] + v.getOrElse(4) { 0 }
        val total = v.sum()
        val dIdle = idle - lastIdle
        val dTotal = total - lastTotal
        lastIdle = idle; lastTotal = total
        if (dTotal <= 0) 0f else ((dTotal - dIdle).toFloat() / dTotal).coerceIn(0f, 1f)
    } catch (e: Exception) {
        0f
    }
}

/** Estimated resource profile for a (backend, mode) choice, grounded in on-device benchmarks. */
data class NodeProfile(
    val computeUnit: String,
    val memImpact: Float,   // 0..1
    val thermal: Float,     // 0..1
    val performance: Float, // 0..1
    val tokPerSec: Int,     // decode tok/s (qwen2.5-0.5B Q8_0, measured/estimated)
    val note: String,
)

fun nodeProfile(backend: String, mode: String, s: Strings): NodeProfile {
    val local = mode == "local_shard"
    return when (backend) {
        "opencl" -> NodeProfile(
            s.t("profile.opencl.unit"), 0.55f, 0.60f,
            if (local) 0.95f else 0.42f, if (local) 72 else 28,
            s.t("profile.opencl.note"),
        )
        "vulkan" -> NodeProfile(
            s.t("profile.vulkan.unit"), 0.55f, 0.55f,
            if (local) 0.72f else 0.46f, if (local) 75 else 32,
            s.t("profile.vulkan.note"),
        )
        else -> NodeProfile(
            s.t("profile.cpu.unit"), 0.35f, 0.85f,
            if (local) 0.78f else 0.55f, if (local) 100 else 39,
            s.t("profile.cpu.note"),
        )
    }
}
