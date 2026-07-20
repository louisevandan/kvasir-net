package ai.banya.linkcpp.wallet

import java.io.InputStream
import java.io.OutputStream
import java.net.InetAddress
import java.net.ServerSocket
import java.net.Socket
import java.net.URI
import java.security.MessageDigest
import java.util.Base64
import javax.net.ssl.SSLSocketFactory
import kotlin.concurrent.thread

/**
 * Relays a ring stage's raw TCP streams to the coordinator through the hub over
 * a WebSocket (443), so a NAT node — or a hub behind Cloudflare, which proxies
 * only 80/443 — needs no publicly reachable ring port. The local ring stage
 * dials 127.0.0.1:proxyPort; each connection is bridged to a WebSocket at
 * {hub}/api/ring-relay, which the hub joins to the coordinator's ring listener.
 * Ring bytes (role preamble, hello, frames) pass through untouched.
 *
 * A minimal RFC 6455 client (handshake + binary frames, client masking) is used
 * so it works on any Android version and over plain ws:// or TLS wss://.
 */
class RingRelay(
    hubBase: String,
    private val controllerId: String,
    private val token: String,
    private val log: (String) -> Unit,
) {
    private val uri = URI(hubBase.replaceFirst("http", "ws").trimEnd('/') + "/api/ring-relay")
    private val tls = uri.scheme == "wss"
    private val host = uri.host
    private val port = if (uri.port > 0) uri.port else if (tls) 443 else 80
    private var server: ServerSocket? = null

    /** Start the local TCP proxy; returns 127.0.0.1 port the stage should dial. */
    fun start(): Int {
        val s = ServerSocket(0, 8, InetAddress.getByName("127.0.0.1"))
        server = s
        thread(name = "ring-relay-accept") {
            while (!s.isClosed) {
                val c = try { s.accept() } catch (e: Exception) { break }
                thread(name = "ring-relay-bridge") { runCatching { bridge(c) } }
            }
        }
        log("ring relay proxy on 127.0.0.1:${s.localPort} -> $uri")
        return s.localPort
    }

    fun stop() { runCatching { server?.close() } }

    private fun bridge(client: Socket) {
        runCatching { client.tcpNoDelay = true }   // ring frames are small + latency-sensitive
        val ws = openWs() ?: run { client.close(); return }
        val cin = client.getInputStream(); val cout = client.getOutputStream()
        val win = ws.getInputStream(); val wout = ws.getOutputStream()
        // stage TCP -> WebSocket (client frames are masked)
        val up = thread(name = "relay-up") {
            val buf = ByteArray(65536)
            try { while (true) { val n = cin.read(buf); if (n < 0) break; wsSend(wout, buf, n) } } catch (_: Exception) {}
            runCatching { ws.close() }; runCatching { client.close() }
        }
        // WebSocket -> stage TCP (respond to control frames to keep it alive)
        try { while (true) { val data = wsRecv(win, wout) ?: break; cout.write(data); cout.flush() } } catch (_: Exception) {}
        runCatching { client.close() }; runCatching { ws.close() }
        runCatching { up.join(500) }
    }

    private fun openWs(): Socket? = runCatching {
        val raw = Socket(host, port)
        runCatching { raw.tcpNoDelay = true }
        val sock = if (tls) (SSLSocketFactory.getDefault() as SSLSocketFactory).createSocket(raw, host, port, true) else raw
        val key = Base64.getEncoder().encodeToString(ByteArray(16).also { java.util.Random().nextBytes(it) })
        val path = uri.rawPath + "?controller_id=$controllerId" + if (token.isNotEmpty()) "&token=$token" else ""
        val req = "GET $path HTTP/1.1\r\nHost: $host\r\nUpgrade: websocket\r\nConnection: Upgrade\r\n" +
            "Sec-WebSocket-Key: $key\r\nSec-WebSocket-Version: 13\r\n\r\n"
        sock.getOutputStream().apply { write(req.toByteArray()); flush() }
        val resp = readHttpHead(sock.getInputStream())
        val accept = Regex("(?i)Sec-WebSocket-Accept:\\s*(\\S+)").find(resp)?.groupValues?.get(1)
        val expect = Base64.getEncoder().encodeToString(
            MessageDigest.getInstance("SHA-1").digest(("${key}258EAFA5-E914-47DA-95CA-C5AB0DC85B11").toByteArray()))
        if (!resp.startsWith("HTTP/1.1 101") || accept != expect) { sock.close(); error("ws handshake failed") }
        sock
    }.getOrNull()

    private fun readHttpHead(input: InputStream): String {
        val sb = StringBuilder()
        while (!sb.endsWith("\r\n\r\n")) { val b = input.read(); if (b < 0) break; sb.append(b.toChar()) }
        return sb.toString()
    }

    private fun wsSend(out: OutputStream, data: ByteArray, len: Int) {
        val header = ArrayList<Byte>(10)
        header.add((0x82).toByte())              // FIN + binary opcode
        val mask = 0x80
        when {
            len < 126 -> header.add((mask or len).toByte())
            len < 65536 -> { header.add((mask or 126).toByte()); header.add((len ushr 8).toByte()); header.add(len.toByte()) }
            // 64-bit extended length: shift over a Long. Int.ushr only uses the
            // low 5 bits of its operand, so `len ushr 56` would wrongly become
            // `len ushr 24` and corrupt the frame length (a ring boundary of
            // >=64 KiB — e.g. the last stage's 593 KB result_output, read in
            // 65536-byte chunks — hits exactly this path). Long.ushr uses 6 bits.
            else -> { header.add((mask or 127).toByte()); val l = len.toLong(); for (i in 7 downTo 0) header.add((l ushr (8 * i)).toByte()) }
        }
        val mk = ByteArray(4).also { java.util.Random().nextBytes(it) }
        synchronized(out) {
            out.write(header.toByteArray()); out.write(mk)
            val masked = ByteArray(len); for (i in 0 until len) masked[i] = (data[i].toInt() xor mk[i and 3].toInt()).toByte()
            out.write(masked); out.flush()
        }
    }

    private fun wsRecv(input: InputStream, out: OutputStream): ByteArray? {
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
                0x2, 0x0 -> return payload      // binary / continuation
                0x8 -> return null               // close
                0x9 -> wsFrame(out, 0xA, payload) // ping -> pong (keep the relay alive)
                else -> {}                       // pong/other: ignore, keep reading
            }
        }
    }

    /** Send a control/data frame (masked, as required for client frames). */
    private fun wsFrame(out: OutputStream, opcode: Int, payload: ByteArray) {
        val mk = ByteArray(4).also { java.util.Random().nextBytes(it) }
        synchronized(out) {
            out.write(0x80 or opcode); out.write(0x80 or payload.size)  // control frames are <126 bytes
            out.write(mk)
            val m = ByteArray(payload.size); for (i in payload.indices) m[i] = (payload[i].toInt() xor mk[i and 3].toInt()).toByte()
            out.write(m); out.flush()
        }
    }

    private fun readN(input: InputStream, n: Int): ByteArray? {
        val buf = ByteArray(n); var off = 0
        while (off < n) { val r = input.read(buf, off, n - off); if (r < 0) return null; off += r }
        return buf
    }
}
