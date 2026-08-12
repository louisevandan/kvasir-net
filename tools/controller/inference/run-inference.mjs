import { ControllerInstance } from '../client/controller-instance.mjs';

const [endpoint, nodeId = 'gpu-0', prompt = 'Reply with one short greeting.', model = 'Qwen2.5-1.5B-Instruct-Q8_0.gguf', concurrentArgument = '1'] = process.argv.slice(2);
const concurrent = Number(concurrentArgument);
if (!Number.isInteger(concurrent) || concurrent < 1 || concurrent > 256) throw new Error('concurrent must be an integer from 1 to 256');
const controller = new ControllerInstance({ controllerId: 'nodejs-controller-example', endpoint });
const deploymentId = `stock-${Date.now()}`;
const bindingId = `stock-binding-${Date.now()}`;

const inventory = await controller.inventory();
console.log(`P4_INVENTORY agent=${inventory.agentId} adapters=${inventory.snapshot.adapters.length} gpus=${inventory.snapshot.gpus.length}`);
const node = await controller.createNode({ nodeId, adapterId: 'llamacpp-stock', nodeSpec: { resource_policy: 'adapter-owned', p4_max_inflight: concurrent } });
console.log(`P4_NODE state=${node.state} id=${node.nodeId}`);
let generation;
try {
  for await (const event of controller.loadModel({ nodeId, deploymentId, bindingId, model, planRevision: 'stock-v1' })) {
    if (event.type === 'load-progress') console.log(`P4_LOAD percent=${event.percent} detail=${event.detail}`);
    if (event.type === 'model-bound') { generation = event.runtimeGeneration; console.log(`P4_BOUND generation=${generation}`); }
  }
  const run = async (index) => {
    let text = '';
    let done;
    for await (const event of controller.infer({ nodeId, deploymentId, bindingId, runtimeGeneration: generation, prompt: `${prompt} Request number ${index + 1}.`, maxTokens: 16, temperature: 0.2, options: { top_p: 0.9, top_k: 20, seed: 7 + index } })) {
      if (event.type === 'accepted') console.log(`P4_ACCEPTED index=${index} session=${event.sessionId}`);
      else if (event.type === 'token') text += event.text;
      else done = event;
    }
    if (!done || !text) throw new Error(`request ${index} did not produce a complete text stream`);
    console.log(`P4_DONE index=${index} session=${done.sessionId} tokens=${done.generatedTokens} text=${JSON.stringify(text)}`);
  };
  await Promise.all(Array.from({ length: concurrent }, (_, index) => run(index)));
} finally {
  if (generation) console.log(`P4_UNBOUND binding=${(await controller.unloadModel({ nodeId, deploymentId, bindingId })).bindingId}`);
}
