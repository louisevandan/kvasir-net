/**
 * The node's side of the relay: one outbound connection, many p4 sockets.
 *
 * The machine running this keeps a single connection open to the relay and
 * listens on nothing. When the relay says a stream has opened, this dials the
 * local p4 agent on loopback and copies bytes between the two, unexamined. From
 * the agent's point of view somebody on the machine connected to it; from the
 * caller's point of view they connected to a public address. Neither is wrong.
 *
 * Reconnecting is the normal case, not the failure case. Laptops sleep, wifi
 * changes, relays restart. The delay backs off to a minute and resets on a
 * successful registration, and every in-flight stream is dropped on the way
 * down — p4 treats a closed hop as a closed hop and does not need help.
 */
import net from 'node:net';
import { TYPE, encode, createParser, typeName } from './protocol.mjs';

const RETRY_MIN_MS = 1_000;
const RETRY_MAX_MS = 60_000;

/**
 * @param {object} options
 * @param {string} options.relayHost   where the relay listens
 * @param {number} options.relayPort
 * @param {string} options.nodeId
 * @param {string} options.owner       the operator wallet, base58
 * @param {(message: string) => Promise<string>} options.sign  base64 signature
 * @param {number} options.agentPort   the local p4 agent, on loopback
 * @param {(event: object) => void} [options.onEvent]
 */
export function connectTunnel(options) {
  const {
    relayHost, relayPort, nodeId, owner, sign,
    agentPort, agentHost = '127.0.0.1', onEvent = () => {},
  } = options;

  let socket = null;
  let retry = RETRY_MIN_MS;
  let stopped = false;
  let timer = null;
  const streams = new Map();          // stream id -> socket to the local agent
  const state = { connected: false, registered: false, advertise: null, lastError: null };

  const emit = (kind, detail) => { try { onEvent({ kind, at: new Date().toISOString(), ...detail }); } catch { /* the caller's problem */ } };

  const send = (type, stream, payload) => {
    if (!socket || socket.destroyed) return false;
    return socket.write(encode(type, stream, payload));
  };

  /** A new inbound p4 connection: open the matching socket to the local agent. */
  function openStream(id) {
    const local = net.connect({ host: agentHost, port: agentPort });
    local.setNoDelay(true);
    streams.set(id, local);

    local.on('connect', () => emit('stream-open', { stream: id }));
    local.on('data', (chunk) => {
      if (!send(TYPE.DATA, id, chunk)) {
        local.pause();
        socket?.once('drain', () => local.resume());
      }
    });
    const finish = (why) => {
      if (!streams.delete(id)) return;
      send(TYPE.CLOSE, id, Buffer.from(why, 'utf8'));
      local.destroy();
      emit('stream-close', { stream: id, why });
    };
    local.on('end', () => finish('agent ended'));
    local.on('close', () => finish('agent closed'));
    // The agent not being up is the ordinary case while the app is starting, so
    // it closes one stream and says why rather than tearing the tunnel down.
    local.on('error', (error) => finish(`agent unreachable: ${error.message}`));
  }

  function toAgent(id, payload) {
    const local = streams.get(id);
    if (!local || local.destroyed) return;
    if (!local.write(payload)) {
      socket?.pause();
      local.once('drain', () => socket?.resume());
    }
  }

  function dropStreams(why) {
    for (const [id, local] of streams) { streams.delete(id); local.destroy(); }
    if (why) emit('streams-dropped', { why });
  }

  function connect() {
    if (stopped) return;
    socket = net.connect({ host: relayHost, port: relayPort });
    socket.setNoDelay(true);

    socket.on('connect', () => {
      state.connected = true;
      state.lastError = null;
      emit('connected', { relay: `${relayHost}:${relayPort}` });
    });

    socket.on('data', createParser(async (type, stream, payload) => {
      switch (type) {
        case TYPE.CHALLENGE: {
          let challenge;
          try { challenge = JSON.parse(payload.toString('utf8')); } catch { return; }
          // The relay names itself in the challenge; sign what it asked for, so
          // the same wallet can hold tunnels on more than one relay.
          const text = `kvasir p4 relay\nrelay: ${challenge.relayHost}\nnode: ${nodeId}\n` +
            `wallet: ${owner}\nnonce: ${challenge.nonce}`;
          let signature;
          try { signature = await sign(text); }
          catch (error) { state.lastError = `could not sign: ${error.message}`; emit('error', { error: state.lastError }); socket.destroy(); return; }
          send(TYPE.HELLO, 0, Buffer.from(JSON.stringify({ nodeId, owner, signature, agentPort }), 'utf8'));
          return;
        }
        case TYPE.WELCOME: {
          let welcome;
          try { welcome = JSON.parse(payload.toString('utf8')); } catch { return; }
          if (!welcome.ok) {
            state.lastError = welcome.error ?? 'refused';
            emit('refused', { error: state.lastError });
            socket.destroy();
            return;
          }
          state.registered = true;
          state.advertise = welcome.advertise;
          retry = RETRY_MIN_MS;
          emit('registered', { advertise: welcome.advertise });
          return;
        }
        case TYPE.OPEN: return openStream(stream);
        case TYPE.DATA: return toAgent(stream, payload);
        case TYPE.CLOSE: {
          const local = streams.get(stream);
          if (local) { streams.delete(stream); local.end(); }
          return;
        }
        case TYPE.PING: return send(TYPE.PONG, 0, payload);
        case TYPE.PONG: return;
        default: emit('error', { error: `unexpected ${typeName(type)} from the relay` });
      }
    }, (error) => {
      state.lastError = error.message;
      emit('error', { error: error.message });
      socket.destroy();
    }));

    const down = (why) => {
      const wasRegistered = state.registered;
      state.connected = false;
      state.registered = false;
      state.advertise = null;
      dropStreams(why);
      if (stopped) return;
      emit('disconnected', { why, retryInMs: retry, wasRegistered });
      timer = setTimeout(connect, retry);
      timer.unref?.();
      retry = Math.min(retry * 2, RETRY_MAX_MS);
    };
    socket.on('error', (error) => { state.lastError = error.message; });
    socket.on('close', () => down(state.lastError ?? 'closed'));
  }

  connect();

  return {
    status: () => ({ ...state, streams: streams.size }),
    stop() {
      stopped = true;
      if (timer) clearTimeout(timer);
      dropStreams('stopped');
      socket?.destroy();
    },
  };
}
