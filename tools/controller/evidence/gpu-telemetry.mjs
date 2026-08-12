import { spawn } from 'node:child_process';

export function startGpuTelemetry() {
  const samples = [];
  let pending = '';
  const process = spawn('nvidia-smi', [
    '--query-gpu=uuid,utilization.gpu,utilization.memory,memory.used,memory.total,power.draw',
    '--format=csv,noheader,nounits', '-lms', '250'
  ], { stdio: ['ignore', 'pipe', 'ignore'], windowsHide: true });
  process.stdout.on('data', (chunk) => {
    const lines = `${pending}${chunk}`.split(/\r?\n/);
    pending = lines.pop() ?? '';
    for (const line of lines) {
      const [uuid, gpu, memory, used, total, power] = line.split(',').map((field) => field.trim());
      if (!uuid || ![gpu, memory, used, total, power].every((field) => Number.isFinite(Number(field)))) continue;
      samples.push({ observed_at: new Date().toISOString(), uuid, gpu_pct: Number(gpu), memory_pct: Number(memory), memory_mib: Number(used), memory_total_mib: Number(total), power_w: Number(power) });
    }
  });
  return {
    async stop() {
      process.kill();
      await new Promise((resolve) => process.once('close', resolve));
      return samples;
    }
  };
}

export function summarizeGpuTelemetry(samples, distribution) {
  const byGpu = new Map();
  for (const sample of samples) byGpu.set(sample.uuid, [...(byGpu.get(sample.uuid) ?? []), sample]);
  return [...byGpu].map(([uuid, values]) => ({
    uuid,
    samples: values.length,
    gpu_pct: distribution(values.map((sample) => sample.gpu_pct)),
    memory_pct: distribution(values.map((sample) => sample.memory_pct)),
    memory_mib: distribution(values.map((sample) => sample.memory_mib)),
    power_w: distribution(values.map((sample) => sample.power_w))
  }));
}
