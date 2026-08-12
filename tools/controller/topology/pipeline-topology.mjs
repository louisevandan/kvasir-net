const csv = (name, fallback) => (process.env[name] ?? fallback).split(',').map((value) => value.trim()).filter(Boolean);

export function ringTopology(stagePortBase) {
  const ids = csv('P4_PIPELINE_NODE_IDS', 'p4-gpu-3090,p4-gpu-4080');
  const gpuUuids = csv('P4_PIPELINE_NODE_GPU_UUIDS', 'GPU-38e6dbac-fee5-ac16-62d4-cfacbe02f8ed,GPU-79caabbe-c843-631f-3cea-9c01e652c78c');
  const vram = csv('P4_PIPELINE_NODE_VRAM_GIB', '24,15.99').map(Number);
  const cores = csv('P4_PIPELINE_NODE_CORES', '24,24').map(Number);
  const hosts = csv('P4_PIPELINE_STAGE_HOSTS', ids.map(() => '127.0.0.1').join(','));
  if (!ids.length || ![gpuUuids, vram, cores, hosts].every((values) => values.length === ids.length) || vram.some((value) => !Number.isFinite(value) || value <= 0) || cores.some((value) => !Number.isInteger(value) || value <= 0)) throw new Error('P4_PIPELINE_NODE_* and P4_PIPELINE_STAGE_HOSTS must describe the same valid node set');
  const remoteAgentEndpoint = process.env.P4_PIPELINE_REMOTE_AGENT_ENDPOINT;
  const remoteGroupHost = process.env.P4_PIPELINE_REMOTE_GROUP_HOST;
  const remoteIds = remoteAgentEndpoint ? csv('P4_PIPELINE_REMOTE_STAGE_NODE_IDS', ids.slice(2).join(',')) : [];
  if (remoteAgentEndpoint && (!remoteGroupHost || !remoteIds.length || remoteIds.some((id) => !ids.includes(id)))) throw new Error('remote P4 Pipeline requires remote agent, group host, and owned node IDs');
  const localIds = ids.filter((id) => !remoteIds.includes(id));
  if (!localIds.length) throw new Error('at least one Pipeline stage must belong to the local agent');
  const nodes = ids.map((id, index) => ({ id, kind: 'local', gpu_uuid: gpuUuids[index], cores: cores[index], node_port: stagePortBase + index, stage_host: hosts[index] }));
  return { nodes, plannerNodes: nodes.map((node, index) => ({ vram: vram[index], ram: 0, cores: node.cores, gpu_uuid: node.gpu_uuid, memory_model: 'dedicated_device' })), localIds, remoteIds, remoteAgentEndpoint, remoteGroupHost, agentStagePlan: (ownedIds, request) => ({ ...request, nodes: nodes.map((node) => ({ ...node, kind: ownedIds.includes(node.id) ? 'local' : 'external' })) }) };
}
