import { randomUUID } from 'node:crypto';
import { agentLink } from './transport/agent-link.mjs';

const KIND = {
  execute: 1, token: 2, done: 3, error: 4, healthCheck: 16, health: 17,
  loadProgress: 19, draftReport: 20, ingressSubmit: 32, ingressAccepted: 33,
  inventoryQuery: 34, hardwareReport: 35, nodeCreate: 38, nodeCreated: 39,
  modelLoad: 40, modelBound: 41, modelUnload: 42, modelUnbound: 43
};

/** A logical controller client. It never sees adapter endpoints or worker processes. */
export class ControllerInstance {
  constructor({ controllerId = randomUUID(), endpoint }) {
    if (!endpoint) throw new Error('endpoint is required');
    this.controllerId = controllerId;
    this.endpoint = endpoint;
    this.link = agentLink(endpoint);
  }

  async inventory({ requestId = randomUUID(), signal, timeoutMs = 0 } = {}) {
    for await (const message of this.#exchange(encodeInventory({ controllerId: this.controllerId, requestId }), { signal, timeoutMs })) {
      if (message.kind === KIND.hardwareReport) return { ...message, snapshot: JSON.parse(message.snapshot) };
      if (message.kind === KIND.error) throw new Error(`P4 inventory ${message.requestId}: ${message.detail}`);
      throw new Error(`unexpected P4 inventory response kind ${message.kind}`);
    }
    throw new Error('agent closed before hardware report');
  }

  async createNode({ nodeId = randomUUID(), adapterId, nodeSpec = {}, operationId = randomUUID(), signal, timeoutMs = 0 }) {
    if (!adapterId) throw new Error('adapterId is required');
    const nodeSpecJson = encodeObject(nodeSpec, 'nodeSpec');
    for await (const message of this.#exchange(encodeNodeCreate({ controllerId: this.controllerId, operationId, nodeId, adapterId, nodeSpecJson }), { signal, timeoutMs })) {
      if (message.kind === KIND.nodeCreated) return message;
      if (message.kind === KIND.error) throw new Error(`P4 node create ${message.requestId}: ${message.detail}`);
      throw new Error(`unexpected P4 node create response kind ${message.kind}`);
    }
    throw new Error('agent closed before node creation');
  }

  async *loadModel({ nodeId, deploymentId, bindingId = randomUUID(), model, planRevision, stagePlan = {}, loadOptions, operationId = randomUUID(), signal, timeoutMs = 0 }) {
    if (!nodeId || !deploymentId || !model || !planRevision) throw new Error('nodeId, deploymentId, model and planRevision are required');
    if (loadOptions !== undefined && Object.hasOwn(stagePlan, 'load_options')) throw new Error('stagePlan.load_options and loadOptions cannot both be supplied');
    // Canonical schema: apps/p4/docs/model-load.md. It remains nested inside
    // bounded stage_plan JSON, so P4B1 framing does not become backend-specific.
    const stagePlanJson = encodeObject(loadOptions === undefined ? stagePlan : { ...stagePlan, load_options: loadOptions }, 'stagePlan');
    for await (const message of this.#exchange(encodeModelLoad({ controllerId: this.controllerId, nodeId, operationId, deploymentId, bindingId, model, planRevision, stagePlanJson }), { signal, timeoutMs })) {
      if (message.kind === KIND.loadProgress) yield { type: 'load-progress', ...message };
      else if (message.kind === KIND.draftReport) yield { type: 'draft-report', ...message };
      else if (message.kind === KIND.modelBound) { yield { type: 'model-bound', ...message }; return; }
      else if (message.kind === KIND.error) throw new Error(`P4 model load ${message.requestId}: ${message.detail}`);
      else throw new Error(`unexpected P4 model load response kind ${message.kind}`);
    }
    throw new Error('agent closed before model binding');
  }

  async unloadModel({ nodeId, deploymentId, bindingId, operationId = randomUUID(), signal, timeoutMs = 0 }) {
    for await (const message of this.#exchange(encodeModelUnload({ controllerId: this.controllerId, nodeId, operationId, deploymentId, bindingId }), { signal, timeoutMs })) {
      if (message.kind === KIND.modelUnbound) return message;
      if (message.kind === KIND.error) throw new Error(`P4 model unload ${message.requestId}: ${message.detail}`);
      throw new Error(`unexpected P4 model unload response kind ${message.kind}`);
    }
    throw new Error('agent closed before model unload');
  }

  async *infer({ nodeId, deploymentId, bindingId, runtimeGeneration, prompt, sessionId = '', requestId = randomUUID(), maxTokens = 128, temperature = 0.7, options = {}, signal, timeoutMs = 0 }) {
    if (!nodeId || !deploymentId || !bindingId || !Number.isSafeInteger(runtimeGeneration) || runtimeGeneration < 1 || typeof prompt !== 'string') throw new Error('nodeId, deploymentId, bindingId, runtimeGeneration and prompt are required');
    const optionJson = encodeObject(options, 'options');
    const ingressId = randomUUID();
    for await (const message of this.#exchange(encodeIngress({ controllerId: this.controllerId, ingressId, requestId, sessionId, nodeId, deploymentId, bindingId, runtimeGeneration, maxTokens, temperature, prompt, optionJson }), { signal, timeoutMs })) {
      if (message.kind === KIND.ingressAccepted) yield { type: 'accepted', ...message };
      else if (message.kind === KIND.token) yield { type: 'token', ...message };
      else if (message.kind === KIND.done) { yield { type: 'done', ...message }; return; }
      else if (message.kind === KIND.error) throw new Error(`P4 request ${message.requestId}: ${message.detail}`);
      else throw new Error(`unexpected P4 inference response kind ${message.kind}`);
    }
    throw new Error('agent closed before DONE');
  }

  async health({ nodeId, requestId = randomUUID(), signal, timeoutMs = 0 } = {}) {
    for await (const message of this.#exchange(encodeHealth({ controllerId: this.controllerId, nodeId, requestId }), { signal, timeoutMs })) {
      if (message.kind === KIND.health) return message;
      if (message.kind === KIND.error) throw new Error(`P4 health ${message.requestId}: ${message.detail}`);
      throw new Error(`unexpected P4 health response kind ${message.kind}`);
    }
    throw new Error('agent closed before health response');
  }

  async *#exchange(packet, options) {
    for await (const frame of this.link.exchange(packet, options)) yield decode(frame);
  }
}

function text(value) { const data = Buffer.from(String(value), 'utf8'); const length = Buffer.allocUnsafe(4); length.writeUInt32LE(data.length); return Buffer.concat([length, data]); }
function u32(value) { const out = Buffer.allocUnsafe(4); out.writeUInt32LE(value); return out; }
function u64(value) { const out = Buffer.allocUnsafe(8); out.writeBigUInt64LE(BigInt(value)); return out; }
function f32(value) { const out = Buffer.allocUnsafe(4); out.writeFloatLE(value); return out; }
function frame(kind, payload, requestId) { return { kind, payload, requestId }; }
function encodeObject(value, name) { if (!value || typeof value !== 'object' || Array.isArray(value)) throw new Error(`${name} must be a JSON object`); const json = JSON.stringify(value); if (Buffer.byteLength(json, 'utf8') > 256 * 1024) throw new Error(`${name} too large`); return json; }
function encodeIngress(v) { return frame(KIND.ingressSubmit, Buffer.concat([text(v.controllerId), text(v.ingressId), text(v.requestId), text(v.sessionId), text(v.nodeId), text(v.deploymentId), text(v.bindingId), u64(v.runtimeGeneration), u32(v.maxTokens), f32(v.temperature), text(v.prompt), text(v.optionJson)]), v.requestId); }
function encodeInventory(v) { return frame(KIND.inventoryQuery, Buffer.concat([text(v.controllerId), text(v.requestId)]), v.requestId); }
function encodeNodeCreate(v) { return frame(KIND.nodeCreate, Buffer.concat([text(v.controllerId), text(v.operationId), text(v.nodeId), text(v.adapterId), text(v.nodeSpecJson)]), v.operationId); }
function encodeModelLoad(v) { return frame(KIND.modelLoad, Buffer.concat([text(v.controllerId), text(v.nodeId), text(v.operationId), text(v.deploymentId), text(v.bindingId), text(v.model), text(v.planRevision), text(v.stagePlanJson)]), v.operationId); }
function encodeModelUnload(v) { return frame(KIND.modelUnload, Buffer.concat([text(v.controllerId), text(v.nodeId), text(v.operationId), text(v.deploymentId), text(v.bindingId)]), v.operationId); }
function encodeHealth(v) { return frame(KIND.healthCheck, Buffer.concat([text(v.controllerId), text(v.nodeId), text(v.requestId)]), v.requestId); }
function decode({ kind, payload }) { let offset = 0; const readText = () => { const length = payload.readUInt32LE(offset); offset += 4; const value = payload.subarray(offset, offset + length).toString('utf8'); offset += length; return value; }; const readU32 = () => { const value = payload.readUInt32LE(offset); offset += 4; return value; }; const readU64 = () => { const value = payload.readBigUInt64LE(offset); offset += 8; return Number(value); };
  if (kind === KIND.ingressAccepted) return { kind, ingressId: readText(), requestId: readText(), sessionId: readText() };
  if (kind === KIND.hardwareReport) return { kind, agentId: readText(), reportId: readText(), snapshot: readText() };
  if (kind === KIND.nodeCreated) return { kind, operationId: readText(), nodeId: readText(), adapterId: readText(), state: readText(), detail: readText() };
  if (kind === KIND.modelBound) return { kind, operationId: readText(), nodeId: readText(), deploymentId: readText(), bindingId: readText(), runtimeGeneration: readU64(), state: readText(), detail: readText() };
  if (kind === KIND.modelUnbound) return { kind, operationId: readText(), nodeId: readText(), deploymentId: readText(), bindingId: readText(), detail: readText() };
  if (kind === KIND.token) { const controllerId = readText(); const nodeId = readText(); const requestId = readText(); const sessionId = readText(); const phase = payload[offset++]; const position = readU32(); const index = readU32(); return { kind, controllerId, nodeId, requestId, sessionId, phase, position, index, text: readText() }; }
  if (kind === KIND.done) { const controllerId = readText(); const nodeId = readText(); const requestId = readText(); const sessionId = readText(); const reason = readText(); return { kind, controllerId, nodeId, requestId, sessionId, reason, generatedTokens: readU32() }; }
  if (kind === KIND.error) return { kind, requestId: readText(), detail: readText() };
  if (kind === KIND.health) return { kind, requestId: readText(), nodeId: readText(), ready: payload[offset++] === 1, detail: readText() };
  if (kind === KIND.loadProgress) return { kind, operationId: readText(), nodeId: readText(), percent: readU32(), detail: readText() };
  if (kind === KIND.draftReport) return { kind, operationId: readText(), nodeId: readText(), modelBytes: readU64(), kvBytes: readU64(), layerBytes: readU64(), ffnBytes: readU64(), detail: readText() };
  return { kind };
}
