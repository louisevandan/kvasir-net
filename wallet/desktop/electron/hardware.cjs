'use strict'
/**
 * What this machine actually is.
 *
 * The node screen used to score a machine from a table keyed on the backend the
 * user picked — a number the app invented and then reported to the settlement
 * service as if it had been measured. A p4 node is admitted on its real
 * capability, so ask the machine instead: which accelerators it has, how much
 * memory they carry, what the CPU is.
 *
 * Probes are cached: they shell out, and nothing here changes while the app runs.
 */
const os = require('node:os')
const { execFile } = require('node:child_process')

const run = (cmd, args, timeout = 4000) => new Promise((resolve) => {
  execFile(cmd, args, { timeout, windowsHide: true }, (error, stdout) => resolve(error ? null : String(stdout)))
})

const MIB = 1024 * 1024

async function nvidia() {
  const out = await run('nvidia-smi', ['--query-gpu=name,memory.total', '--format=csv,noheader,nounits'])
  if (!out) return []
  return out.trim().split('\n').filter(Boolean).map((line) => {
    const [name, mib] = line.split(',').map((part) => part.trim())
    return { name, memoryBytes: Number(mib) * MIB, backend: 'cuda' }
  })
}

async function amd() {
  // rocm-smi's JSON shape moves between releases; read the two fields we need.
  const out = await run('rocm-smi', ['--showproductname', '--showmeminfo', 'vram', '--json'])
  if (!out) return []
  try {
    const parsed = JSON.parse(out)
    return Object.entries(parsed)
      .filter(([key]) => /^card/i.test(key))
      .map(([, card]) => ({
        name: card['Card Series'] || card['Card model'] || card['Card SKU'] || 'AMD GPU',
        memoryBytes: Number(card['VRAM Total Memory (B)'] ?? 0) || null,
        backend: 'rocm',
      }))
  } catch { return [] }
}

async function apple() {
  const out = await run('system_profiler', ['SPDisplaysDataType', '-json'], 8000)
  const unified = os.totalmem()          // Apple Silicon shares memory with the GPU
  if (!out) return [{ name: 'Apple GPU', memoryBytes: unified, backend: 'metal' }]
  try {
    const items = JSON.parse(out).SPDisplaysDataType ?? []
    return items.map((item) => ({
      name: item.sppci_model || item._name || 'Apple GPU',
      memoryBytes: unified,
      backend: 'metal',
    }))
  } catch { return [{ name: 'Apple GPU', memoryBytes: unified, backend: 'metal' }] }
}

async function windowsGpus() {
  const out = await run('powershell', ['-NoProfile', '-Command',
    '(Get-CimInstance Win32_VideoController | Select-Object Name,AdapterRAM | ConvertTo-Json -Compress)'], 8000)
  if (!out) return []
  try {
    const parsed = JSON.parse(out)
    const list = Array.isArray(parsed) ? parsed : [parsed]
    return list.map((gpu) => ({ name: gpu.Name, memoryBytes: Number(gpu.AdapterRAM) || null, backend: null }))
  } catch { return [] }
}

let cached = null

/**
 * @returns {Promise<{os:string, arch:string, cpu:{brand:string, cores:number},
 *   ramBytes:number, backend:'metal'|'cuda'|'rocm'|'cpu', gpus:Array<{name:string,memoryBytes:number|null,backend:string|null}>}>}
 */
async function capability({ refresh = false } = {}) {
  if (cached && !refresh) return cached
  const platform = os.platform()
  let gpus = []
  if (platform === 'darwin') gpus = await apple()
  else {
    gpus = await nvidia()
    if (!gpus.length) gpus = await amd()
    if (!gpus.length && platform === 'win32') gpus = await windowsGpus()
  }
  // The backend is what the hardware says, not what a dropdown was left on.
  const backend = gpus.find((gpu) => gpu.backend)?.backend
    ?? (platform === 'darwin' && os.arch() === 'arm64' ? 'metal' : 'cpu')
  cached = {
    os: platform === 'darwin' ? 'macos' : platform === 'win32' ? 'windows' : 'linux',
    arch: os.arch(),
    cpu: { brand: os.cpus()[0]?.model?.trim() ?? 'unknown', cores: os.cpus().length },
    ramBytes: os.totalmem(),
    backend,
    gpus,
  }
  return cached
}

module.exports = { capability }
