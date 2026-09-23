'use strict'
/**
 * The tunnel that makes this machine dialable.
 *
 * A p4 node is reached by somebody opening a TCP connection to it. There is no
 * route table and no hole punching, and a connection the agent opened itself
 * will not carry work inbound — p4 closes it with "unexpected frame on outbound
 * hop". A laptop behind NAT therefore cannot join a pipeline at all, which is
 * most laptops, which is most of the people this app is for.
 *
 * So the app keeps one outbound connection to a relay that does have a public
 * address, and work dialled at that address arrives down the connection we
 * already hold. The agent still binds loopback and this machine still listens
 * on nothing.
 *
 * This is a port of `tools/p4-relay/tunnel.mjs`, which is the canonical
 * implementation and where the protocol is documented. It is duplicated rather
 * than imported because everything in `electron/` is CommonJS and the app
 * should not grow a build step to reach one module.
 *
 * Signing happens here, in main, with the unlocked wallet — the same key the
 * rewards ledger is keyed on. A locked wallet means no tunnel, which is correct
 * and is reported rather than retried.
 */
const net = require('node:net')

const MAGIC = Buffer.from('KVR1', 'ascii')
const HEADER = 13
const MAX_PAYLOAD = 4 * 1024 * 1024
const T = { HELLO: 1, WELCOME: 2, OPEN: 3, DATA: 4, CLOSE: 5, PING: 6, PONG: 7, CHALLENGE: 8 }
const RETRY_MIN = 1000
const RETRY_MAX = 60000

function encode(type, stream, payload) {
  const body = payload ? (Buffer.isBuffer(payload) ? payload : Buffer.from(String(payload))) : Buffer.alloc(0)
  if (body.length > MAX_PAYLOAD) throw new Error(`frame payload ${body.length} too large`)
  const frame = Buffer.allocUnsafe(HEADER + body.length)
  MAGIC.copy(frame, 0)
  frame.writeUInt8(type, 4)
  frame.writeUInt32BE(stream, 5)
  frame.writeUInt32BE(body.length, 9)
  body.copy(frame, HEADER)
  return frame
}

/** TCP gives no message boundaries; buffer until a whole frame is present. */
function parser(onFrame, onError) {
  let buf = Buffer.alloc(0)
  let broken = false
  return (chunk) => {
    if (broken) return
    buf = buf.length ? Buffer.concat([buf, chunk]) : chunk
    for (;;) {
      if (buf.length < HEADER) return
      if (!buf.subarray(0, 4).equals(MAGIC)) { broken = true; return onError(new Error('framing lost')) }
      const length = buf.readUInt32BE(9)
      if (length > MAX_PAYLOAD) { broken = true; return onError(new Error('frame too large')) }
      if (buf.length < HEADER + length) return
      const type = buf.readUInt8(4)
      const stream = buf.readUInt32BE(5)
      const payload = buf.subarray(HEADER, HEADER + length)
      buf = buf.subarray(HEADER + length)
      onFrame(type, stream, payload)
    }
  }
}

class RelayTunnel {
  constructor() {
    this.socket = null
    this.streams = new Map()
    this.retry = RETRY_MIN
    this.timer = null
    this.stopped = true
    this.options = null
    this.state = { enabled: false, connected: false, registered: false, advertise: null, lastError: null }
    this.log = []
  }

  _note(line) {
    this.log.push(`${new Date().toISOString().slice(11, 19)} ${line}`)
    if (this.log.length > 50) this.log.splice(0, this.log.length - 50)
  }

  status() { return { ...this.state, streams: this.streams.size, log: this.log.slice(-12) } }

  start(options) {
    this.stop()
    this.stopped = false
    this.options = options
    this.state.enabled = true
    this.retry = RETRY_MIN
    this._connect()
    return this.status()
  }

  stop() {
    this.stopped = true
    this.state.enabled = false
    this.state.connected = false
    this.state.registered = false
    this.state.advertise = null
    if (this.timer) { clearTimeout(this.timer); this.timer = null }
    this._dropStreams()
    if (this.socket) { this.socket.destroy(); this.socket = null }
  }

  _send(type, stream, payload) {
    if (!this.socket || this.socket.destroyed) return false
    return this.socket.write(encode(type, stream, payload))
  }

  _dropStreams() {
    for (const [, local] of this.streams) local.destroy()
    this.streams.clear()
  }

  /** A caller reached our public address: open the matching socket to our agent. */
  _openStream(id) {
    const { agentPort, agentHost = '127.0.0.1' } = this.options
    const local = net.connect({ host: agentHost, port: agentPort })
    local.setNoDelay(true)
    this.streams.set(id, local)
    local.on('data', (chunk) => {
      if (!this._send(T.DATA, id, chunk)) {
        local.pause()
        if (this.socket) this.socket.once('drain', () => local.resume())
      }
    })
    const finish = (why) => {
      if (!this.streams.delete(id)) return
      this._send(T.CLOSE, id, Buffer.from(why))
      local.destroy()
    }
    local.on('end', () => finish('agent ended'))
    local.on('close', () => finish('agent closed'))
    // The agent not being up yet is ordinary while the app starts: close the one
    // stream and say so, rather than tearing down a tunnel that is otherwise fine.
    local.on('error', (error) => finish(`agent unreachable: ${error.message}`))
  }

  _toAgent(id, payload) {
    const local = this.streams.get(id)
    if (!local || local.destroyed) return
    if (!local.write(payload)) {
      if (this.socket) this.socket.pause()
      local.once('drain', () => { if (this.socket) this.socket.resume() })
    }
  }

  _connect() {
    if (this.stopped) return
    const { relayHost, relayPort, nodeId, owner, sign } = this.options
    const socket = net.connect({ host: relayHost, port: relayPort })
    this.socket = socket
    socket.setNoDelay(true)

    socket.on('connect', () => {
      this.state.connected = true
      this.state.lastError = null
      this._note(`connected to ${relayHost}:${relayPort}`)
    })

    socket.on('data', parser(async (type, stream, payload) => {
      if (type === T.CHALLENGE) {
        let challenge
        try { challenge = JSON.parse(payload.toString('utf8')) } catch { return }
        const text = `kvasir p4 relay\nrelay: ${challenge.relayHost}\nnode: ${nodeId}\n` +
          `wallet: ${owner}\nnonce: ${challenge.nonce}`
        let signature
        try { signature = await sign(text) } catch (error) {
          this.state.lastError = `cannot sign: ${error.message}`
          this._note(this.state.lastError)
          socket.destroy()
          return
        }
        this._send(T.HELLO, 0, Buffer.from(JSON.stringify({ nodeId, owner, signature, agentPort: this.options.agentPort })))
        return
      }
      if (type === T.WELCOME) {
        let welcome
        try { welcome = JSON.parse(payload.toString('utf8')) } catch { return }
        if (!welcome.ok) {
          this.state.lastError = welcome.error || 'refused'
          this._note(`refused: ${this.state.lastError}`)
          socket.destroy()
          return
        }
        this.state.registered = true
        this.state.advertise = welcome.advertise
        this.retry = RETRY_MIN
        this._note(`reachable at ${welcome.advertise}`)
        return
      }
      if (type === T.OPEN) return this._openStream(stream)
      if (type === T.DATA) return this._toAgent(stream, payload)
      if (type === T.CLOSE) {
        const local = this.streams.get(stream)
        if (local) { this.streams.delete(stream); local.end() }
        return
      }
      if (type === T.PING) return this._send(T.PONG, 0, payload)
    }, (error) => {
      this.state.lastError = error.message
      this._note(error.message)
      socket.destroy()
    }))

    socket.on('error', (error) => { this.state.lastError = error.message })
    socket.on('close', () => {
      if (this.socket === socket) this.socket = null
      this.state.connected = false
      this.state.registered = false
      this.state.advertise = null
      this._dropStreams()
      if (this.stopped) return
      // Sleeping laptops and changing networks are the normal case here, so a
      // disconnect is a wait rather than a fault.
      this._note(`disconnected (${this.state.lastError || 'closed'}), retrying in ${Math.round(this.retry / 1000)}s`)
      this.timer = setTimeout(() => this._connect(), this.retry)
      if (this.timer.unref) this.timer.unref()
      this.retry = Math.min(this.retry * 2, RETRY_MAX)
    })
  }
}

module.exports = { RelayTunnel }
