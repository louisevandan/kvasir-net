#!/usr/bin/env node
/** Ask each agent named in the plan for its node snapshots. */
import { readFileSync } from 'node:fs';
import { connect } from './wire.js';

/**
 * Hand the connection back instead of dropping it.
 *
 * An agent releases a connection's slot only when the owner sends FINISH; a
 * socket that simply dies leaves the return route in place and the semaphore
 * permit held for good (transport.rs:1076-1078). Every run of this tool sends
 * an INSPECT, so every run used to cost the agent a slot it never got back --
 * which is why "one INSPECT costs one slot" was the rule of thumb. It was never
 * the INSPECT; it was the close.
 */
async function release(client, ms = 3_000) {
  try { await client.finish(ms); } catch { client.close?.(); }
}

const plan = JSON.parse(readFileSync(process.argv[2] ?? 'load-plan.step37.json', 'utf8'));
for (const agent of new Set(plan.stages.map((s) => s.agent))) {
  const [host, port] = agent.replace(/^tcp:\/\//, '').split(':');
  const client = await connect({
    host, port: Number(port), address: agent,
    channel: `kvr-inspect-${Date.now().toString(16)}`, deadlineMs: 30_000,
    connectTimeoutMs: 10_000,
  });
  // timeoutMs, not deadlineMs: exchange() reads timeoutMs and falls back to the
  // connection's deadlineMs. Passing deadlineMs here did nothing -- the request
  // ran on the 30 s connection deadline while the code said 20.
  const snapshot = await client.inspect(agent, { timeoutMs: 20_000 });
  console.log(`${agent}`);
  console.log(JSON.stringify(snapshot, null, 2).split('\n').map((l) => '  ' + l).join('\n'));
  await release(client);
}
