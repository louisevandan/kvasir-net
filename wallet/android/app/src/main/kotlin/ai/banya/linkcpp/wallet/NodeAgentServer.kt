package ai.banya.linkcpp.wallet

import android.content.Context
import android.os.Build
import org.json.JSONArray
import org.json.JSONObject
import java.io.File
import java.net.ServerSocket
import java.net.Socket
import java.net.URL
import kotlin.concurrent.thread

/**
 * The phone-side managed node agent: the Android counterpart of the iOS
 * AgentControlServer and controller/nodeagent.py. It speaks the hub's control
 * protocol over a raw HTTP server and, unlike iOS, drives the data plane by
 * spawning the bundled native binaries (linkcpp-node ring stage, ggml-rpc-server)
 * — Android can exec, so this is a real on-demand node.
 */
class NodeAgentServer(private val ctx: Context) {
    val agentPort = 9101
    val rpcPort = 50072

    @Volatile var running = false; private set
    @Volatile var lastEvent = ""; private set
    @Volatile var boundController: String? = null; private set

    // Connection status for the node dashboard: the hubs this node polls and
    // whether it is actively serving a shard/expert range for one right now.
    val knownHubUrls: List<String> get() = knownHubs.keys.toList()
    val serving: Boolean get() =
        stageProc != null || rpcProc != null || activeRelay != null || expertWorker.serving

    // Autonomous shard participation: the phone proactively polls the hub's
    // scarcity/demand market and advertises which under-covered window it is
    // ready to serve (and its reward multiplier). The hub still force-places the
    // node and triggers the partial download; this is the bottom-up signal.
    @Volatile var autonomousShard = true
    @Volatile var shardIntent: JSONObject? = null; private set
    // Layer budget offered to the hub's matchmaker; it clips the scarce gap to a
    // coverable sub-window, so this only bounds how much this phone volunteers.
    @Volatile var shardLayerBudget = 8

    // Multi-hub: the node polls EVERY hub it knows, not just the one that joined
    // it. Hubs are learned from an inbound join/download (LAN hubs, no token) and
    // configured explicitly via POST /control/hubs (remote public hubs, which the
    // node can only reach outbound and which are usually auth-gated). base URL ->
    // service token ("" = none). Per-hub shard_intent is tracked separately.
    private val knownHubs = java.util.concurrent.ConcurrentHashMap<String, String>()
    // MoE expert-parallel participation (runs alongside the layer-shard poll):
    // volunteer for scarce expert ranges on every known hub and serve them.
    private val expertWorker = ExpertWorker(ctx, { knownHubs.toMap() }, { m -> log(m) })
    private val shardIntents = java.util.concurrent.ConcurrentHashMap<String, JSONObject>()
    @Volatile private var enrolling = false
    @Volatile private var activeRelay: RingRelay? = null

    private var server: ServerSocket? = null
    private var stageProc: Process? = null
    private var rpcProc: Process? = null
    private var reportUrl: String? = null
    // Hub base URL for the autonomous shard poll. Captured from any hub-origin
    // URL we're handed — the join report_url or a model download source — since
    // a binding restored after a hub restart re-stages without re-joining, so
    // report_url alone is not reliably present.
    @Volatile private var hubBaseUrl: String? = null
    private var serviceToken: String? = null
    private var owner = ""
    private var desiredLoad: JSONObject? = null
    private val logLines = ArrayDeque<String>()
    private var lastIntentKey = ""

    // Native binaries + libs are shipped in jniLibs and extracted here, executable.
    private val libDir: String get() = ctx.applicationInfo.nativeLibraryDir
    // Bundled libs + the device's OpenCL/Vulkan drivers in /vendor (Adreno's
    // libOpenCL.so lives there and is needed by libggml-opencl).
    private val ldPath: String get() = "$libDir:/vendor/lib64:/system/lib64"
    private fun nodeBin() = File(libDir, "liblinkcpp-node.so")
    private fun rpcBin() = File(libDir, "libggml-rpc-server.so")
    val modelsDir: File get() = File(ctx.filesDir, "models").apply { mkdirs() }
    // Node-serving artifacts (ring-stage windows) live in a subdirectory so they
    // never show up in the "모델 관리" list, which enumerates only top-level GGUFs.
    // They are partial shards, not runnable standalone models.
    private val shardsDir: File get() = File(modelsDir, "shards").apply { mkdirs() }
    private val workerLog: File get() = File(ctx.filesDir, "worker.log")

    fun start(owner: String) {
        this.owner = owner
        if (server != null) return
        loadConfiguredHubs()
        try {
            server = ServerSocket(agentPort)
            running = true
            log("control server listening on :$agentPort")
            thread(name = "kvasir-agent") { acceptLoop() }
            thread(name = "kvasir-shard-poll") { shardPollLoop() }
            expertWorker.start()
        } catch (e: Exception) { log("listen failed: $e") }
    }

    fun stop() {
        running = false
        try { server?.close() } catch (_: Exception) {}
        server = null
        expertWorker.stop()
        stopStage(); stopRpc(); stopRelay()
        log("control server stopped")
    }

    private fun log(s: String) {
        lastEvent = s
        synchronized(logLines) {
            logLines.addLast("${System.currentTimeMillis()} $s")
            while (logLines.size > 400) logLines.removeFirst()
        }
    }

    private fun acceptLoop() {
        while (running) {
            val sock = try { server?.accept() ?: break } catch (e: Exception) { if (running) log("accept: $e"); break }
            thread { handle(sock) }
        }
    }

    // ---- HTTP plumbing (minimal, one request per connection) ------------------

    private fun handle(sock: Socket) {
        sock.use {
            val input = it.getInputStream()
            val head = StringBuilder()
            var prev = 0
            // Read headers until CRLFCRLF.
            while (true) {
                val b = input.read(); if (b == -1) return
                head.append(b.toChar())
                if (b == '\n'.code && prev == '\r'.code && head.endsWith("\r\n\r\n")) break
                prev = b
            }
            val headText = head.toString()
            val requestLine = headText.lineSequence().firstOrNull().orEmpty()
            val parts = requestLine.split(" ")
            if (parts.size < 2) return
            val method = parts[0]
            val path = parts[1].substringBefore("?")
            val contentLength = Regex("(?i)content-length:\\s*(\\d+)").find(headText)?.groupValues?.get(1)?.toIntOrNull() ?: 0
            val body = ByteArray(contentLength)
            var read = 0
            while (read < contentLength) {
                val n = input.read(body, read, contentLength - read); if (n < 0) break; read += n
            }
            val json = runCatching { if (body.isNotEmpty()) JSONObject(String(body)) else JSONObject() }.getOrDefault(JSONObject())
            val (status, payload) = route(method, path, json)
            val out = payload.toString().toByteArray()
            val resp = "HTTP/1.1 $status ${if (status == 200) "OK" else "Error"}\r\n" +
                "Content-Type: application/json\r\nContent-Length: ${out.size}\r\nConnection: close\r\n\r\n"
            it.getOutputStream().apply { write(resp.toByteArray()); write(out); flush() }
        }
    }

    private fun route(method: String, path: String, json: JSONObject): Pair<Int, JSONObject> = when {
        method == "GET" && (path == "/control/status" || path == "/info" || path == "/status") -> 200 to info()
        method == "POST" && path == "/control/join" -> {
            boundController = json.optString("controller_id", null)
            reportUrl = json.optString("report_url", null)
            captureHubBase(reportUrl)
            json.optString("service_token", "").takeIf { it.isNotEmpty() }?.let { serviceToken = it }
            log("joined ${boundController ?: "?"}")
            200 to JSONObject().put("joined", true).put("status", info())
        }
        method == "POST" && path == "/bind" -> {
            boundController = json.optString("controller_id", null); log("bound ${boundController ?: "?"}")
            200 to JSONObject().put("bound", true).put("status", info())
        }
        method == "POST" && path == "/unbind" -> {
            stopStage(); stopRpc(); stopRelay(); desiredLoad = null; boundController = null; reportUrl = null
            log("unbound; released"); 200 to JSONObject().put("unbound", true).put("released", true)
        }
        method == "GET" && path == "/control/proxy/runtime" -> 200 to JSONObject()
            .put("runtime_mode", ringCatalogEntry()).put("installed_packs", JSONArray()).put("install_enabled", false)
        method == "POST" && path == "/control/proxy/stage/start" -> stageStart(json)
        method == "GET" && path == "/control/proxy/stage/status" -> 200 to stageStatus()
        method == "POST" && path == "/control/proxy/stage/stop" -> { stopStage(); 200 to JSONObject().put("stopped", true) }
        method == "POST" && path == "/control/load" -> loadRpc(json)
        method == "POST" && (path == "/control/unload" || path == "/control/load/cancel") -> {
            stopStage(); stopRpc(); stopRelay(); desiredLoad = null; 200 to JSONObject().put("unloaded", true).put("status", info())
        }
        method == "POST" && path == "/control/download" -> downloadModel(json)
        method == "POST" && path == "/control/hubs" -> {
            val url = json.optString("url", "")
            if (url.isEmpty()) 400 to err("url required")
            else { addHub(url, json.optString("token", "")); 200 to JSONObject().put("added", true).put("hubs", JSONArray(knownHubs.keys.toList())) }
        }
        method == "GET" && path == "/control/hubs" -> 200 to JSONObject()
            .put("hubs", JSONArray(knownHubs.keys.toList())).put("shard_intents", JSONObject(shardIntents as Map<*, *>))
        method == "POST" && path == "/control/autonomous" -> {
            autonomousShard = json.optBoolean("enabled", true)
            200 to JSONObject().put("autonomous", autonomousShard)
        }
        method == "POST" && path == "/control/self-enroll" -> {
            val hub = json.optString("hub", ""); val model = json.optString("model", "")
            val ctrlId = json.optString("controller_id", "")
            if (hub.isEmpty() || model.isEmpty()) 400 to err("hub + model required")
            else { thread(name = "kvasir-enroll") { runCatching { selfEnroll(hub, model, ctrlId) }.onFailure { log("self-enroll: $it") } }
                   200 to JSONObject().put("started", true) }
        }
        method == "GET" && path == "/control/logs" -> 200 to JSONObject()
            .put("log", synchronized(logLines) { logLines.takeLast(200).joinToString("\n") })
            .put("worker_running", rpcProc?.isAlive == true)
        else -> 404 to JSONObject().put("error", "unknown path $path")
    }

    // ---- status payload (mirrors nodeagent._info) -----------------------------

    private fun stagedModels(): JSONArray =
        JSONArray().apply { shardsDir.listFiles { f -> f.name.endsWith(".gguf") }?.forEach { put(it.name) } }

    private fun ringInfo(): JSONObject = runCatching {
        JSONObject(String(ProcessBuilder(nodeBin().absolutePath, "--runtime-info")
            .apply { environment()["LD_LIBRARY_PATH"] = ldPath }.redirectErrorStream(true)
            .start().inputStream.readBytes()).trim())
    }.getOrDefault(JSONObject())

    private fun ringCatalogEntry(): JSONObject {
        val id = ringInfo()
        return JSONObject()
            .put("id", "ring_proxy").put("label", "linkcpp Proxy").put("stability", "preview")
            .put("available", true).put("description", "In-app ring stage (bundled)")
            .put("protocol", id.optString("protocol", "linkcpp-stage-v1"))
            .put("adapter_abi", id.optInt("adapter_abi", 0))
            .put("build_id", id.optString("build_id", "unknown"))
            .put("state_snapshot", true).put("chunked_state", true)
    }

    private fun info(): JSONObject {
        val rt = Runtime.getRuntime()
        val ramGib = (rt.maxMemory().toDouble() / (1 shl 30)).coerceAtLeast(2.0)
        val cores = rt.availableProcessors()
        val backend = if (File(libDir, "libggml-opencl.so").exists()) "opencl" else "cpu"
        val resources = JSONObject()
            .put("vram_total_gib", 0.0).put("vram_used_gib", 0.0)
            .put("vram_budget_gib", 4.0).put("ram_total_gib", ramGib)
            .put("ram_used_gib", 0.0).put("ram_budget_gib", (ramGib * 0.4))
            .put("cores_total", cores).put("cores_budget", (cores - 2).coerceAtLeast(2))
            .put("cpu_used_percent", 0.0).put("disk_free_gib", 0.0)
        val runtime = JSONObject()
            .put("unit_version", "0.0.7").put("runtime_pack_version", "0.0.7")
            .put("llama_cpp_version", ringInfo().optString("build_id", "unknown").removePrefix("ring-"))
            .put("rpc_abi", "llama.cpp-rpc").put("llama_cpp_backend", backend)
        return JSONObject()
            .put("node_id", DeviceNode.nodeId(ctx)).put("owner", owner)
            .put("hostname", Build.MODEL).put("name", Build.MODEL)
            .put("host_platform", JSONObject().put("system", "android").put("machine", "arm64").put("hostname", Build.MODEL))
            .put("gpu", "${Build.MANUFACTURER} GPU").put("gpu_uuid", "android-${DeviceNode.nodeId(ctx)}")
            .put("vram_budget_gib", 4.0).put("ram_budget_gib", resources.getDouble("ram_budget_gib"))
            .put("cores", resources.getInt("cores_budget")).put("resources", resources)
            .put("bound_to", boundController).put("rpc_port", rpcPort)
            // "working" for either data plane: an RPC worker (rpcProc) OR a ring
            // stage (stageProc). The hub's shard-coverage rollup counts a node as
            // a live replica only when worker_running is true, so a ring-staging
            // phone must report true or its layer window looks uncovered.
            .put("worker_running", rpcProc?.isAlive == true || stageProc?.isAlive == true)
            .put("models", stagedModels()).put("desired_load", desiredLoad)
            .put("shard_intent", shardIntent ?: JSONObject.NULL)
            .put("known_hubs", JSONArray(knownHubs.keys.toList()))
            .put("shard_intents", JSONObject(shardIntents as Map<*, *>))
            .put("runtime", runtime)
            .put("backend", JSONObject().put("backend_kind", backend)
                .put("backend_runtime_version", Build.VERSION.RELEASE).put("backend_device", "${Build.MANUFACTURER} GPU"))
            .put("capabilities", JSONObject().put("managed", true).put("download", true)
                .put("load", true).put("unload", true).put("reports", reportUrl != null)
                .put("native_agent", true).put("host_system", "android").put("backend_kind", backend))
    }

    // ---- ring stage + rpc worker (spawned) ------------------------------------

    private fun stageStart(json: JSONObject): Pair<Int, JSONObject> {
        val model = json.optString("model", "")
        val layers = json.optJSONArray("layers") ?: return 400 to err("layers required")
        val role = json.optString("role", "")
        val listen = json.optInt("listen_port", 0)
        val next = json.optString("next_endpoint", "")
        // NAT traversal: this phone is reachable only outbound, so the hub tells
        // it to dial its predecessor (dial_prev_endpoint) rather than waiting for
        // an inbound connection it could never accept.
        val dialPrev = json.optString("dial_prev_endpoint", "")
        val acceptNext = json.optBoolean("accept_next", false)
        if (json.optBoolean("coordinator", false)) return 400 to err("phone cannot be the ring coordinator")
        val modelFile = File(shardsDir, File(model).name)
        if (!modelFile.exists()) return 404 to err("model file is not present on node")
        stopStage()
        val backend = if (File(libDir, "libggml-opencl.so").exists()) "99" else "0"
        val cmd = mutableListOf(nodeBin().absolutePath, "--model", modelFile.absolutePath,
            "--layers", "${layers.getInt(0)}:${layers.getInt(1)}", "--role", role,
            "--listen", listen.toString(), "--next", next,
            "--gpu-layers", json.optInt("gpu_layers", backend.toInt()).toString(),
            "--ctx", json.optInt("ctx", 2048).toString(),
            "--parallel", json.optInt("parallel", 1).toString(),
            "--cache-type-k", json.optString("cache_type_k", "f16"),
            "--cache-type-v", json.optString("cache_type_v", "f16"))
        if (!json.optBoolean("kv_offload", true)) cmd += "--no-kv-offload"
        if (dialPrev.isNotEmpty()) { cmd += "--dial-prev"; cmd += dialPrev }
        if (acceptNext) cmd += "--accept-next"
        stageProc = spawn(cmd)
        desiredLoad = JSONObject().put("model", model).put("layers", layers).put("stage", true)
            .put("role", role).put("listen_port", listen).put("next_endpoint", next).put("coordinator", false)
        log("ring stage started: $role [${layers.getInt(0)},${layers.getInt(1)}) :$listen -> $next")
        return 200 to JSONObject().put("accepted", true).put("pid", 0).put("log", workerLog.absolutePath).put("status", stageStatus())
    }

    private fun stageStatus(): JSONObject = JSONObject()
        .put("running", stageProc?.isAlive == true)
        .put("exit_code", if (stageProc?.isAlive == true) JSONObject.NULL else (stageProc?.exitValue() ?: JSONObject.NULL))
        .put("desired_load", desiredLoad)
        .put("log", workerLog.takeIf { it.exists() }?.readLines()?.takeLast(160)?.joinToString("\n") ?: "")

    private fun loadRpc(json: JSONObject): Pair<Int, JSONObject> {
        if (rpcProc?.isAlive != true) {
            rpcProc = spawn(listOf(rpcBin().absolutePath, "-H", "0.0.0.0", "-p", rpcPort.toString()))
            log("rpc worker started on :$rpcPort")
        }
        desiredLoad = json
        val op = json.optString("op_id", "load-${System.nanoTime()}")
        return 200 to JSONObject().put("accepted", true).put("op_id", op).put("rpc_port", rpcPort).put("status", info())
    }

    private fun spawn(cmd: List<String>): Process =
        ProcessBuilder(cmd).apply {
            environment()["LD_LIBRARY_PATH"] = ldPath
            redirectErrorStream(true)
            redirectOutput(ProcessBuilder.Redirect.appendTo(workerLog))
        }.start()

    // Note: stopStage() must NOT stop the relay — stageStart() calls stopStage()
    // first, and selfEnroll() starts the relay just before stageStart(). The
    // relay is torn down explicitly on unbind/unload instead.
    private fun stopStage() { stageProc?.destroy(); stageProc = null }
    private fun stopRelay() { activeRelay?.stop(); activeRelay = null }
    private fun stopRpc() { rpcProc?.destroy(); rpcProc = null }

    // ---- autonomous shard participation (phone -> hub demand market) ----------

    /** Best hub base URL we've captured (join report_url or a download source). */
    private fun hubBase(): String? =
        (hubBaseUrl ?: reportUrl)?.substringBefore("/api/")?.takeIf { it.startsWith("http") }

    /** Remember the hub's base URL from any hub-origin URL we're handed. */
    private fun captureHubBase(url: String?) {
        if (url == null) return
        val base = url.substringBefore("/api/")
        if (base.startsWith("http") && base != url) {
            hubBaseUrl = base
            knownHubs.putIfAbsent(base, "")   // LAN hubs authenticate by origin, no token
        }
    }

    /** Public entry: the app registers a hub (with a wallet-auth token) for the
     *  node to poll. Used after an in-app SIWS + 2FA sign-in to a remote hub. */
    fun registerHub(url: String, token: String) = addHub(url, token)

    /** Hubs this node currently polls, for the settings UI. */
    fun knownHubList(): List<String> = knownHubs.keys.toList()

    /** Register a hub the node should poll (e.g. a remote public hub reachable
     *  only outbound). Persisted so the node keeps polling it across restarts. */
    private fun addHub(url: String, token: String) {
        val base = url.substringBefore("/api/").trimEnd('/')
        if (!base.startsWith("http")) return
        knownHubs[base] = token
        runCatching {
            val prefs = ctx.getSharedPreferences("kvasir-node", Context.MODE_PRIVATE)
            val arr = JSONArray()
            knownHubs.forEach { (u, t) -> arr.put(JSONObject().put("url", u).put("token", t)) }
            prefs.edit().putString("configuredHubs", arr.toString()).apply()
        }
        log("hub registered: $base${if (token.isNotEmpty()) " (auth)" else ""}")
    }

    private fun loadConfiguredHubs() {
        runCatching {
            val prefs = ctx.getSharedPreferences("kvasir-node", Context.MODE_PRIVATE)
            val arr = JSONArray(prefs.getString("configuredHubs", "[]"))
            for (i in 0 until arr.length()) {
                val o = arr.getJSONObject(i)
                val u = o.optString("url", ""); if (u.isNotEmpty()) knownHubs[u] = o.optString("token", "")
            }
        }
    }

    /**
     * Poll the hub's shard demand market and volunteer for the scarcest window.
     * The hub replies with the model + layer window whose coverage is lowest
     * (highest reward), which we advertise via info().shard_intent. Placement and
     * the partial download of that window are still driven by the hub force-place
     * path; this is the bottom-up availability signal that lets the market self-heal.
     */
    private fun shardPollLoop() {
        while (running) {
            try { Thread.sleep(45_000) } catch (_: InterruptedException) { break }
            if (!running) break
            if (!autonomousShard) { shardIntents.clear(); updateBestIntent(); continue }
            // Poll EVERY known hub outbound and volunteer to each independently —
            // a node can detect and offer to serve on multiple hubs at once.
            for ((base, token) in knownHubs) {
                val body = JSONObject()
                    .put("node_id", DeviceNode.nodeId(ctx))
                    .put("max_layers", shardLayerBudget)
                val resp = httpPostJson("$base/api/shard-volunteer", body, token.ifEmpty { serviceToken ?: "" })
                if (resp != null && resp.optBoolean("assigned", false)) {
                    val intent = JSONObject()
                        .put("hub", base)
                        .put("model", resp.optString("model", ""))
                        .put("layers", resp.optJSONArray("layers") ?: JSONArray())
                        .put("scarcity", resp.optDouble("scarcity", 0.0))
                        .put("replicas", resp.optInt("replicas", 0))
                        .put("target", resp.optInt("target", 0))
                        .put("reward_multiplier", 1.0 + resp.optDouble("scarcity", 0.0))
                    shardIntents[base] = intent
                    logIntentChange(base, intent)
                    // Autonomous participation: a hub we can only reach outbound
                    // (auth-gated, i.e. has a token) won't force-place us, so if
                    // we're idle, self-enroll to serve the window it offered.
                    if (token.isNotEmpty() && !enrolling
                        && stageProc?.isAlive != true && rpcProc?.isAlive != true) {
                        val model = intent.optString("model")   // selfEnroll is single-flight
                        thread(name = "kvasir-autoenroll") {
                            runCatching { selfEnroll(base, model) }.onFailure { log("auto-enroll: $it") }
                        }
                    }
                } else if (resp != null) {
                    if (shardIntents.remove(base) != null) log("shard intent @ $base: none (coverage at target)")
                }
            }
            updateBestIntent()
        }
    }

    /** The advertised primary intent = the highest-reward one across all hubs. */
    private fun updateBestIntent() {
        shardIntent = shardIntents.values.maxByOrNull { it.optDouble("scarcity", 0.0) }
    }

    private fun logIntentChange(base: String, intent: JSONObject) {
        val l = intent.optJSONArray("layers")
        val key = "$base:${intent.optString("model")}:$l"
        if (key != lastIntentKey) {
            lastIntentKey = key
            log("shard intent @ $base: ${File(intent.optString("model")).name} " +
                "[${l?.optInt(0)},${l?.optInt(1)}) scarcity=${intent.optDouble("scarcity")} " +
                "reward=${intent.optDouble("reward_multiplier")}x")
        }
    }

    /**
     * Self-enroll to a hub reachable only outbound (e.g. a remote public hub):
     * declare intent to serve a shard, pull the stage config the hub plans for
     * us, download just our window, and self-start the ring stage dialing the
     * coordinator. The hub never calls back — the whole lifecycle is node-driven.
     */
    fun selfEnroll(hubBase: String, model: String, controllerId: String = ""): JSONObject {
        // Single-flight: never enroll twice concurrently or while already serving,
        // or two stages race and restart each other.
        synchronized(this) {
            if (enrolling || stageProc?.isAlive == true) return err("enroll/serve already active")
            enrolling = true
        }
        try {
            return selfEnrollInner(hubBase, model, controllerId)
        } finally {
            enrolling = false   // re-entry is then blocked by the stageProc.isAlive check
        }
    }

    private fun selfEnrollInner(hubBase: String, model: String, controllerId: String): JSONObject {
        val base = hubBase.substringBefore("/api/").trimEnd('/')
        val token = knownHubs[base] ?: ""
        val backend = if (File(libDir, "libggml-opencl.so").exists()) "opencl" else "cpu"
        val enrollBody = JSONObject()
            .put("node_id", DeviceNode.nodeId(ctx)).put("name", Build.MODEL).put("model", model)
            .put("controller_id", controllerId)
            .put("host_platform", JSONObject().put("system", "android").put("machine", "arm64"))
            .put("backend", JSONObject().put("backend_kind", backend))
            .put("vram_budget_gib", 4.0).put("ram_budget_gib", 4.0)
            .put("cores", Runtime.getRuntime().availableProcessors())
            .put("stage_port", 51072).put("ctx", 512)
        val enr = httpPostJson("$base/api/shard-enroll", enrollBody, token)
            ?: return err("enroll request failed")
        if (!enr.optBoolean("enrolled", false)) return err("not enrolled: $enr")
        log("self-enrolled to $base (controller ${enr.optString("controller_id")})")
        var resp: JSONObject? = null
        for (i in 0 until 40) {
            Thread.sleep(2000)
            val r = httpGetJson("$base/api/shard-enroll/config?node_id=${DeviceNode.nodeId(ctx)}", token)
            if (r != null && r.optBoolean("ready", false)) { resp = r; break }
        }
        val rr = resp ?: return err("stage config not ready")
        val c = rr.optJSONObject("config") ?: return err("stage config missing")
        val layers = c.optJSONArray("layers") ?: return err("config missing layers")
        val name = File(model).name
        val dest = File(shardsDir, name)
        val url = "$base/api/proxy/models/$name/stage?layers=${layers.getInt(0)}:${layers.getInt(1)}"
        log("self-enroll: downloading window [${layers.getInt(0)},${layers.getInt(1)})")
        if (!httpDownload(url, token, dest)) return err("shard download failed")
        // Relay: the hub can only be reached over 443, so bridge the ring stream
        // through a WebSocket instead of dialing the coordinator's port directly.
        rr.optJSONObject("relay")?.let { relay ->
            stopRelay()
            val rl = RingRelay(base, relay.optString("controller_id"), token, ::log)
            val proxyEp = "127.0.0.1:${rl.start()}"
            activeRelay = rl
            if (c.optString("dial_prev_endpoint").isNotEmpty()) c.put("dial_prev_endpoint", proxyEp)
            c.put("next_endpoint", proxyEp)
        }
        val (status, payload) = stageStart(c)   // self-start; dials the relay proxy
        return if (status == 200) JSONObject().put("serving", true).put("layers", layers) else payload
    }

    private fun httpGetJson(url: String, token: String = ""): JSONObject? = runCatching {
        val conn = (URL(url).openConnection() as java.net.HttpURLConnection).apply {
            connectTimeout = 8000; readTimeout = 8000
            if (token.isNotEmpty()) setRequestProperty("Authorization", "Bearer $token")
        }
        val text = (if (conn.responseCode in 200..299) conn.inputStream else conn.errorStream)
            ?.readBytes()?.let { String(it) } ?: ""
        if (conn.responseCode in 200..299 && text.isNotEmpty()) JSONObject(text) else null
    }.getOrNull()

    private fun httpDownload(url: String, token: String, dest: File): Boolean = runCatching {
        val conn = (URL(url).openConnection() as java.net.HttpURLConnection).apply {
            connectTimeout = 15000; readTimeout = 120000
            if (token.isNotEmpty()) setRequestProperty("Authorization", "Bearer $token")
        }
        if (conn.responseCode !in 200..299) return false
        val tmp = File(dest.parentFile, "${dest.name}.part")
        conn.inputStream.use { input -> tmp.outputStream().use { input.copyTo(it, 1 shl 20) } }
        tmp.renameTo(dest)
        log("shard downloaded: ${dest.name} (${dest.length()} bytes)")
        true
    }.getOrDefault(false)

    private fun httpPostJson(url: String, body: JSONObject, token: String = ""): JSONObject? = runCatching {
        val conn = (URL(url).openConnection() as java.net.HttpURLConnection).apply {
            requestMethod = "POST"; connectTimeout = 8000; readTimeout = 8000; doOutput = true
            setRequestProperty("Content-Type", "application/json")
            // Wallet-authenticated hubs read a session/node token from the
            // Authorization header (see hub _authed_wallet); a static M2M service
            // token is also accepted on its own header. Send whichever we have.
            if (token.isNotEmpty()) {
                setRequestProperty("Authorization", "Bearer $token")
                setRequestProperty("x-linkcpp-service-token", token)
            }
        }
        conn.outputStream.use { it.write(body.toString().toByteArray()) }
        val text = (if (conn.responseCode in 200..299) conn.inputStream else conn.errorStream)
            ?.readBytes()?.let { String(it) } ?: ""
        if (conn.responseCode in 200..299 && text.isNotEmpty()) JSONObject(text) else null
    }.getOrNull()

    // ---- model download (hub -> phone staging) --------------------------------

    private val downloading = HashSet<String>()

    private fun downloadModel(json: JSONObject): Pair<Int, JSONObject> {
        val model = json.optString("model", "")
        val url = json.optJSONObject("source")?.optString("url", "") ?: ""
        if (model.isEmpty() || url.isEmpty()) return 400 to err("download requires model + source.url")
        captureHubBase(url)
        val op = json.optString("op_id", "dl-${System.nanoTime()}")
        val name = File(model).name
        val dest = File(shardsDir, name)
        if (dest.exists()) return 200 to JSONObject().put("accepted", true).put("op_id", op).put("already_present", true)
        synchronized(downloading) { if (!downloading.add(name)) return 200 to JSONObject().put("accepted", true).put("in_progress", true) }
        log("downloading $model")
        thread(name = "kvasir-dl") {
            runCatching {
                val tmp = File(shardsDir, "$name.part")
                (URL(url).openConnection()).apply { connectTimeout = 15000; readTimeout = 60000 }
                    .getInputStream().use { input -> tmp.outputStream().use { input.copyTo(it, 1 shl 20) } }
                tmp.renameTo(dest)
                log("download complete: $model")
            }.onFailure { log("download failed: $it"); File(shardsDir, "$name.part").delete() }
            synchronized(downloading) { downloading.remove(name) }
        }
        return 200 to JSONObject().put("accepted", true).put("op_id", op)
    }

    private fun err(m: String) = JSONObject().put("error", m)
}

object DeviceNode {
    fun nodeId(ctx: Context): String {
        val id = android.provider.Settings.Secure.getString(ctx.contentResolver,
            android.provider.Settings.Secure.ANDROID_ID) ?: "device"
        return "android-${id.take(8)}"
    }
}
