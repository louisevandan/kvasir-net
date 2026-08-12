const SCHEMA = 'p4.adapter-capability/v1';
const REQUIRED_FIELDS = ['protocol', 'adapter_abi', 'capability_bits'];

export function compatibleRingCapabilities(inventories, adapterId = 'adapter-local') {
  const capabilities = inventories.map(({ endpoint, agentId, snapshot }) => {
    const adapter = snapshot.adapters.find((candidate) => candidate.adapter_id === adapterId);
    if (!adapter) throw new Error(`P4 Pipeline capability missing: agent=${agentId} endpoint=${endpoint} adapter=${adapterId}`);
    let descriptor;
    try { descriptor = JSON.parse(adapter.descriptor); } catch { throw new Error(`P4 Pipeline capability descriptor is invalid JSON: agent=${agentId}`); }
    const runtime = descriptor.pipeline_runtime;
    if (descriptor.schema !== SCHEMA || !runtime?.available) {
      throw new Error(`P4 Pipeline capability unavailable: agent=${agentId} detail=${runtime?.detail ?? 'descriptor schema mismatch'}`);
    }
    for (const field of REQUIRED_FIELDS) {
      if (runtime[field] === undefined || runtime[field] === null) throw new Error(`P4 Pipeline capability missing ${field}: agent=${agentId}`);
    }
    return { endpoint, agentId, protocol: runtime.protocol, adapterAbi: runtime.adapter_abi, capabilityBits: runtime.capability_bits, runtimeContract: runtime.runtime_contract ?? null, buildId: runtime.build_id ?? null };
  });
  const baseline = capabilities[0];
  const mismatches = capabilities.slice(1).flatMap((candidate) => REQUIRED_FIELDS.flatMap((field) => {
    const key = field === 'adapter_abi' ? 'adapterAbi' : field === 'capability_bits' ? 'capabilityBits' : field;
    return candidate[key] === baseline[key] ? [] : [`${field}: ${baseline[key]} != ${candidate[key]} (${candidate.agentId})`];
  }));
  if (mismatches.length) throw new Error(`P4 incompatible native Pipeline capability; model load was not attempted: ${mismatches.join('; ')}`);
  return capabilities;
}
