const occupancyKeys = ['active', 'in_flight', 'peak', 'limit', 'capacity'];

const asText = (entry) => typeof entry === 'string' ? entry : entry?.message ?? entry?.text ?? '';

export const numericFields = (line) => Object.fromEntries(
  [...line.matchAll(/([a-z0-9_]+)=([0-9]+(?:\.[0-9]+)?)/g)]
    .map(([, key, value]) => [key, Number(value)])
);

export function buildGroupStats(group, nodes) {
  const stages = (group.processes ?? []).map((process, stageIndex) => {
    const messages = (process.logs ?? []).map(asText);
    const latest = (marker) => [...messages].reverse()
      .find((message) => message.includes(marker)) ?? '';
    const wire = latest('[linker_wire_summary]');
    const request = latest('[linker_request_summary]');
    const aggregate = numericFields(latest('[linker_stage_aggregate]'));
    const wavefront = numericFields(latest('[linker_scheduler] op=wavefront'));
    const nodeId = process.identity?.nodeId ?? nodes[stageIndex]?.id;
    const node = nodes.find((candidate) => candidate.id === nodeId);
    const computeFrames = aggregate.compute_frames ?? 0;
    const occupancy = Object.fromEntries(
      occupancyKeys.filter((key) => wavefront[key] !== undefined)
        .map((key) => [key, wavefront[key]])
    );
    return {
      stage_index: process.identity?.stageIndex ?? stageIndex,
      node_id: nodeId,
      gpu_uuid: node?.gpu_uuid,
      phase: process.phase,
      shared_memory: {
        sent_frames: numericFields(wire).local_hidden_send_frames ?? 0,
        received_frames: numericFields(wire).local_hidden_recv_frames ?? 0
      },
      wire: numericFields(wire),
      request: numericFields(request),
      aggregate,
      tokens_per_frame: computeFrames ? aggregate.batched_tokens / computeFrames : null,
      occupancy: Object.keys(occupancy).length ? occupancy : null
    };
  });
  const transfers = (group.observability?.transfers ?? []).map((transfer) => ({
    request_id: transfer.request_id,
    total_bytes: transfer.total_bytes,
    elapsed_ms: transfer.elapsed_ms,
    average_bytes_per_second: transfer.average_bytes_per_second,
    source_to_destination: transfer.source_to_destination
  }));
  return {
    group_phase: group.phase,
    pipeline_occupancy: stages.find((stage) => stage.occupancy)?.occupancy ?? null,
    stages,
    transfers
  };
}
