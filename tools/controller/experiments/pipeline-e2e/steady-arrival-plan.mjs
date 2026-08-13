import { performance } from 'node:perf_hooks';

export function steadyArrivalPlan(totalRequests, initialRequests, intervalMs) {
  if (!Number.isInteger(totalRequests) || totalRequests < 1) throw new Error('totalRequests must be a positive integer');
  if (!Number.isInteger(initialRequests) || initialRequests < 1 || initialRequests > totalRequests) {
    throw new Error('initialRequests must be an integer from 1 to totalRequests');
  }
  if (!Number.isInteger(intervalMs) || intervalMs < 0) throw new Error('intervalMs must be a non-negative integer');
  if (totalRequests > initialRequests && intervalMs === 0) {
    throw new Error('intervalMs must be positive when requests arrive after the initial cohort');
  }
  return Array.from({ length: totalRequests }, (_, index) => ({
    phase: index < initialRequests ? 'initial' : 'steady',
    scheduled_after_ms: index < initialRequests ? 0 : (index - initialRequests + 1) * intervalMs
  }));
}

export async function executeWindow(requests, window, execute) {
  const results = Array(requests.length);
  let next = 0;
  const worker = async () => {
    while (next < requests.length) {
      const index = next++;
      results[index] = await execute({ ...requests[index], scheduled_at: performance.now() });
    }
  };
  await Promise.all(Array.from({ length: Math.min(window, requests.length) }, worker));
  return results;
}

export async function executeSteadyArrivals(requests, execute, hasActiveInference) {
  const started = performance.now();
  return Promise.all(requests.map(async (request) => {
    const scheduledAt = started + request.arrival.scheduled_after_ms;
    const delay = scheduledAt - performance.now();
    if (delay > 0) await new Promise((resolve) => setTimeout(resolve, delay));
    if (request.arrival.phase === 'steady' && !hasActiveInference()) {
      throw new Error('steady arrival did not overlap an active inference request');
    }
    return execute({ ...request, scheduled_at: scheduledAt });
  }));
}
