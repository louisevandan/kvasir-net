import { test } from 'node:test';
import assert from 'node:assert/strict';
import { executeSteadyArrivals, steadyArrivalPlan } from './steady-arrival-plan.mjs';

test('keeps the initial cohort simultaneous and spaces later ingress', () => {
  assert.deepEqual(steadyArrivalPlan(6, 3, 250), [
    { phase: 'initial', scheduled_after_ms: 0 },
    { phase: 'initial', scheduled_after_ms: 0 },
    { phase: 'initial', scheduled_after_ms: 0 },
    { phase: 'steady', scheduled_after_ms: 250 },
    { phase: 'steady', scheduled_after_ms: 500 },
    { phase: 'steady', scheduled_after_ms: 750 }
  ]);
});

test('rejects an unstaggered follow-up cohort', () => {
  assert.throws(() => steadyArrivalPlan(5, 4, 0), /intervalMs must be positive/);
});

test('rejects a follow-up submitted after every inference request has ended', async () => {
  await assert.rejects(
    executeSteadyArrivals(
      [{ arrival: { phase: 'steady', scheduled_after_ms: 0 } }],
      async () => undefined,
      () => false
    ),
    /did not overlap an active inference request/
  );
});
