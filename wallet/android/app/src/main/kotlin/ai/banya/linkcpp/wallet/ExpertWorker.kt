package ai.banya.linkcpp.wallet

import android.content.Context
import android.os.Build
import org.json.JSONArray
import org.json.JSONObject
import java.io.File
import java.net.HttpURLConnection
import java.net.InetAddress
import java.net.Socket
import java.net.URI
import java.net.URL
import java.security.MessageDigest
import java.util.Base64
import javax.net.ssl.SSLSocketFactory
import kotlin.concurrent.thread

/**
 * Autonomous MoE expert-worker loop for a phone — the Android port of
 * scripts/expert-worker-agent.py (M3 of docs/design/moe-expert-sharding.md).
 *
 *   volunteer -> download the assigned expert shard -> serve it with the bundled
 *   linkcpp-expert-worker binary -> bridge its dispatch stream to the backbone
 *   over /api/expert-relay (443, NAT-friendly) -> heartbeat coverage.
 *
 * Model-agnostic by contract: the phone hardcodes NOTHING about a model. The
 * hub carries the per-model dims — `n_embd` (required by the worker to serve),
 * `n_layer`, `n_expert` — in the /api/expert-volunteer response (or the slice
 * GGUF carries n_embd and the worker reads it). If the response omits n_embd the
 * assignment is skipped with a clear log rather than guessing.
 */
class ExpertWorker(
    private val ctx: Context,
    private val knownHubs: () -> Map<String, String>,   // base URL -> node token
    private val log: (String) -> Unit,
) {
    @Volatile private var running = false
    // True while an expert range is actively being served (worker process alive).
    val serving: Boolean get() = worker != null
    @Volatile private var worker: Process? = null
    @Volatile private var relay: ExpertRelayDial? = null
    @Volatile private var enrolling = false
    private var pollThread: Thread? = null
    private val servePort = 52800
    private val maxExperts = 32          // experts/layer budget offered; hub clips to the scarce gap

    private val libDir get() = ctx.applicationInfo.nativeLibraryDir
    private val ldPath get() = "$libDir:/vendor/lib64:/system/lib64"
    private fun workerBin() = File(libDir, "liblinkcpp-expert-worker.so")
    private val shardsDir get() = File(ctx.filesDir, "expert-shards").apply { mkdirs() }

    fun start() {
        if (running || !workerBin().exists()) { if (!workerBin().exists()) log("expert worker binary missing"); return }
        running = true
        pollThread = thread(name = "kvasir-expert-poll") { pollLoop() }
    }

    fun stop() {
        running = false
        pollThread?.interrupt(); pollThread = null
        worker?.destroy(); worker = null
        relay?.stop(); relay = null
    }

    fun serving(): Boolean = worker?.isAlive == true

    private fun pollLoop() {
        while (running) {
            try { Thread.sleep(45_000) } catch (_: InterruptedException) { break }
            if (!running) break
            if (worker?.isAlive == true) continue      // already serving; keep the assignment
            for ((base, token) in knownHubs()) {
                if (!running || worker?.isAlive == true) break
                // Volunteer for ANY under-covered model (model="" => hub picks the scarcest).
                val body = JSONObject().put("model", "").put("max_experts", maxExperts)
                val resp = postJson("$base/api/expert-volunteer", body, token) ?: continue
                if (!resp.optBoolean("assigned", false)) continue
                if (token.isEmpty() || enrolling) continue
                enrolling = true
                try { selfEnroll(base, token, resp) }
                catch (e: Exception) { log("expert enroll @ $base: $e") }
                finally { enrolling = false }
            }
        }
    }

    private fun selfEnroll(base: String, token: String, a: JSONObject) {
        val model = a.optString("model")
        val layer = a.optInt("layer", -1)
        val experts = a.optJSONArray("experts") ?: return
        val e0 = experts.optInt(0, -1); val e1 = experts.optInt(1, -1)
        // Model-agnostic dims: MUST come from the network, never hardcoded.
        val nEmbd = a.optInt("n_embd", 0)
        val nLayer = a.optInt("n_layer", 0)
        val nExpert = a.optInt("n_expert", 0)
        if (model.isEmpty() || layer < 0 || e0 < 0 || e1 <= e0) { log("bad expert assignment: $a"); return }
        if (nEmbd <= 0) {
            log("hub did not supply n_embd for '$model' — cannot serve (hub must carry per-model dims). skipping.")
            return
        }

        // 1) fetch just our expert range (cached across restarts)
        val name = File(model).name
        val slice = File(shardsDir, "$name.L${layer}_e%03d-%03d.gguf".format(e0, e1))
        if (!slice.exists()) {
            val url = "$base/api/proxy/models/$name/expert-shard?layers=$layer:${layer + 1}&experts=$e0:$e1"
            log("expert: downloading L$layer e[$e0,$e1) -> ${slice.name}")
            if (!download(url, token, slice)) { slice.delete(); return }
        }

        // 2) bridge the dispatch stream over the hub relay (443). The worker LISTENS
        //    on servePort; the dial connects that port + the hub WS and pipes.
        val session = "expert-${DeviceNode.nodeId(ctx)}"
        relay?.stop()
        relay = ExpertRelayDial(base, session, token, servePort, log).also { it.start() }

        // 3) serve it — dims come from the assignment, not from code.
        val cmd = listOf(workerBin().absolutePath, "--model", slice.absolutePath,
            "--serve", servePort.toString(), "--layer", layer.toString(), "--n-embd", nEmbd.toString())
        log("expert: starting worker ${cmd.joinToString(" ")}")
        worker = ProcessBuilder(cmd).apply {
            environment()["LD_LIBRARY_PATH"] = ldPath
            redirectErrorStream(true)
            redirectOutput(ProcessBuilder.Redirect.appendTo(File(ctx.filesDir, "expert-worker.log")))
        }.start()

        // 4) heartbeat coverage while the worker lives
        val cov = JSONObject()
            .put("worker_id", DeviceNode.nodeId(ctx)).put("model", name)
            .put("n_layer", nLayer).put("n_expert", nExpert)
            .put("segments", JSONArray().put(JSONArray().put(layer).put(e0).put(e1)))
            .put("url", "relay:$session")
        while (running && worker?.isAlive == true) {
            postJson("$base/api/expert-coverage", cov, token)
            try { Thread.sleep(15_000) } catch (_: InterruptedException) { break }
        }
        log("expert worker exited (rc=${worker?.exitValue()})")
        relay?.stop(); relay = null
    }

    // ---- HTTP -----------------------------------------------------------------

    private fun postJson(url: String, body: JSONObject, token: String): JSONObject? = runCatching {
        val c = (URL(url).openConnection() as HttpURLConnection).apply {
            requestMethod = "POST"; connectTimeout = 12000; readTimeout = 30000; doOutput = true
            setRequestProperty("Content-Type", "application/json")
            // Node tokens are verified from the Authorization bearer (hub
            // _bearer_or_cookie); the M2M header covers a static service token.
            if (token.isNotEmpty()) {
                setRequestProperty("Authorization", "Bearer $token")
                setRequestProperty("X-Linkcpp-Service-Token", token)
            }
        }
        c.outputStream.use { it.write(body.toString().toByteArray()) }
        if (c.responseCode !in 200..299) return null
        JSONObject(c.inputStream.bufferedReader().use { it.readText() })
    }.getOrNull()

    private fun download(url: String, token: String, dest: File): Boolean = runCatching {
        val c = (URL(url).openConnection() as HttpURLConnection).apply {
            connectTimeout = 12000; readTimeout = 600000
            if (token.isNotEmpty()) {
                setRequestProperty("Authorization", "Bearer $token")
                setRequestProperty("X-Linkcpp-Service-Token", token)
            }
        }
        if (c.responseCode !in 200..299) return false
        val tmp = File(dest.absolutePath + ".part")
        c.inputStream.use { input -> tmp.outputStream().use { input.copyTo(it) } }
        tmp.renameTo(dest)
    }.getOrDefault(false)
}

/**
 * Worker-mode dial-out bridge: pipe the local expert-worker --serve TCP port to
 * the hub's /api/expert-relay WebSocket, so a NAT phone reaches the backbone
 * over 443. Android port of expert-relay-dial.py (--mode worker); minimal
 * RFC 6455 client (masked binary frames out, ping answered) like RingRelay.kt.
 */
class ExpertRelayDial(
    hubBase: String,
    private val session: String,
    private val token: String,
    private val localPort: Int,
    private val log: (String) -> Unit,
) {
    private val uri = URI(hubBase.replaceFirst("http", "ws").trimEnd('/') + "/api/expert-relay")
    private val tls = uri.scheme == "wss"
    private val host = uri.host
    private val port = if (uri.port > 0) uri.port else if (tls) 443 else 80
    @Volatile private var stopped = false
    private var local: Socket? = null
    private var ws: Socket? = null

    fun start() = thread(name = "expert-relay-dial") { runCatching { loop() }.onFailure { log("expert-relay: $it") } }.let {}

    fun stop() { stopped = true; runCatching { local?.close() }; runCatching { ws?.close() } }

    private fun loop() {
        while (!stopped) {
            val l = runCatching { Socket("127.0.0.1", localPort).apply { tcpNoDelay = true } }.getOrNull()
            val w = if (l != null) openWs() else null
            if (l == null || w == null) { runCatching { l?.close() }; try { Thread.sleep(3000) } catch (_: Exception) { break }; continue }
            local = l; ws = w
            val cin = l.getInputStream(); val cout = l.getOutputStream()
            val win = w.getInputStream(); val wout = w.getOutputStream()
            val up = thread(name = "expert-up") {
                val buf = ByteArray(65536)
                try { while (true) { val n = cin.read(buf); if (n < 0) break; wsSend(wout, buf, n) } } catch (_: Exception) {}
                runCatching { w.close() }; runCatching { l.close() }
            }
            try { while (true) { val d = wsRecv(win, wout) ?: break; cout.write(d); cout.flush() } } catch (_: Exception) {}
            runCatching { l.close() }; runCatching { w.close() }; runCatching { up.join(500) }
            if (!stopped) try { Thread.sleep(2000) } catch (_: Exception) { break }
        }
    }

    private fun openWs(): Socket? = runCatching {
        val raw = Socket(host, port).apply { tcpNoDelay = true }
        val sock = if (tls) (SSLSocketFactory.getDefault() as SSLSocketFactory).createSocket(raw, host, port, true) else raw
        val key = Base64.getEncoder().encodeToString(ByteArray(16).also { java.util.Random().nextBytes(it) })
        val path = uri.rawPath + "?session=$session" + if (token.isNotEmpty()) "&token=$token" else ""
        val req = "GET $path HTTP/1.1\r\nHost: $host\r\nUpgrade: websocket\r\nConnection: Upgrade\r\n" +
            "Sec-WebSocket-Key: $key\r\nSec-WebSocket-Version: 13\r\n\r\n"
        sock.getOutputStream().apply { write(req.toByteArray()); flush() }
        val resp = StringBuilder()
        val ins = sock.getInputStream()
        while (!resp.endsWith("\r\n\r\n")) { val b = ins.read(); if (b < 0) break; resp.append(b.toChar()) }
        val accept = Regex("(?i)Sec-WebSocket-Accept:\\s*(\\S+)").find(resp)?.groupValues?.get(1)
        val expect = Base64.getEncoder().encodeToString(
            MessageDigest.getInstance("SHA-1").digest(("${key}258EAFA5-E914-47DA-95CA-C5AB0DC85B11").toByteArray()))
        if (!resp.startsWith("HTTP/1.1 101") || accept != expect) { sock.close(); error("ws handshake failed") }
        sock
    }.getOrNull()

    private fun wsSend(out: java.io.OutputStream, data: ByteArray, len: Int) {
        val header = ArrayList<Byte>(10)
        header.add((0x82).toByte())
        val mask = 0x80
        when {
            len < 126 -> header.add((mask or len).toByte())
            len < 65536 -> { header.add((mask or 126).toByte()); header.add((len ushr 8).toByte()); header.add(len.toByte()) }
            else -> { header.add((mask or 127).toByte()); val l = len.toLong(); for (i in 7 downTo 0) header.add((l ushr (8 * i)).toByte()) }
        }
        val mk = ByteArray(4).also { java.util.Random().nextBytes(it) }
        synchronized(out) {
            out.write(header.toByteArray()); out.write(mk)
            val m = ByteArray(len); for (i in 0 until len) m[i] = (data[i].toInt() xor mk[i and 3].toInt()).toByte()
            out.write(m); out.flush()
        }
    }

    private fun wsRecv(input: java.io.InputStream, out: java.io.OutputStream): ByteArray? {
        while (true) {
            val b0 = input.read(); if (b0 < 0) return null
            val opcode = b0 and 0x0f
            val b1 = input.read(); if (b1 < 0) return null
            val masked = (b1 and 0x80) != 0
            var len = (b1 and 0x7f).toLong()
            if (len == 126L) len = (readN(input, 2) ?: return null).fold(0L) { a, x -> (a shl 8) or (x.toLong() and 0xff) }
            else if (len == 127L) len = (readN(input, 8) ?: return null).fold(0L) { a, x -> (a shl 8) or (x.toLong() and 0xff) }
            val mk = if (masked) readN(input, 4) ?: return null else null
            val payload = readN(input, len.toInt()) ?: return null
            if (mk != null) for (i in payload.indices) payload[i] = (payload[i].toInt() xor mk[i and 3].toInt()).toByte()
            when (opcode) {
                0x2, 0x0 -> return payload
                0x8 -> return null
                0x9 -> { // ping -> pong (masked)
                    val m = ByteArray(4).also { java.util.Random().nextBytes(it) }
                    synchronized(out) {
                        out.write(0x8A); out.write(0x80 or payload.size); out.write(m)
                        val mm = ByteArray(payload.size); for (i in payload.indices) mm[i] = (payload[i].toInt() xor m[i and 3].toInt()).toByte()
                        out.write(mm); out.flush()
                    }
                }
                else -> {}
            }
        }
    }

    private fun readN(input: java.io.InputStream, n: Int): ByteArray? {
        val buf = ByteArray(n); var off = 0
        while (off < n) { val r = input.read(buf, off, n - off); if (r < 0) return null; off += r }
        return buf
    }
}
