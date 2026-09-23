#!/usr/bin/env node
/** Ask each agent named in the plan for its node snapshots. */
import { readFileSync } from 'node:fs';
import { connect } from './wire.js';

const plan = JSON.parse(readFileSync(process.argv[2] ?? 'load-plan.step37.json', 'utf8'));
for (const agent of new Set(plan.stages.map((s) => s.agent))) {
  const [host, port] = agent.replace(/^tcp:\/\//, '').split(':');
  const client = await connect({
    host, port: Number(port), address: agent,
    channel: `kvr-inspect-${Date.now().toString(16)}`, deadlineMs: 30_000,
  });
  const snapshot = await client.inspect(agent, { deadlineMs: 20_000 });
  console.log(`${agent}`);
  console.log(JSON.stringify(snapshot, null, 2).split('\n').map((l) => '  ' + l).join('\n'));
  client.close?.();
}
