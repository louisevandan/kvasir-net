'use strict'
/**
 * Which compute engines this machine can actually run.
 *
 * hardware.cjs answers "what is this machine" — GPUs, RAM, CPU. That is not
 * the same as "can it do work". A machine with an RTX card and no executor
 * binary on disk looks capable on the node screen and computes nothing, which
 * is exactly how a dev build's "start node" ran nothing on the compute side
 * without anyone noticing.
 *
 * So this probes the two things capability() does not:
 *
 *   - executors: is each engine's binary present, and does the OS actually
 *     load it? A binary that is on disk but cannot start (wrong architecture,
 *     a CUDA runtime DLL missing) is the common failure, and it fails in a way
 *     that is easy to misread as "the node is idle".
 *   - GPU readiness: free VRAM, driver and CUDA versions. Free VRAM is live
 *     rather than cached, because this GPU is shared — another local service
 *     (e.g. an image generator) can take most of it while the app runs.
 *
 * Nothing here decides placement. It reports what is true so the operator,
 * and the node screen, can tell "ready", "missing" and "installed but broken"
 * apart.
 */
const fs = require('node:fs')
const path = require('node:path')
const { spawn, execFile } = require('node:child_process')

const MIB = 1024 * 1024
const PROBE_TIMEOUT_MS = 5000

// Windows NTSTATUS codes a process exits with when the loader refuses it
// before main() runs. They are the difference between "not installed" and
// "installed, but this machine cannot load it" — worth naming precisely.
const NTSTATUS = {
  3221225781: 'a required DLL is missing (STATUS_DLL_NOT_FOUND) — usually the CUDA runtime',
  3221225595: 'the binary is built for a different architecture (STATUS_INVALID_IMAGE_FORMAT)',
  3221225785: 'a required DLL has the wrong entry points (STATUS_ENTRYPOINT_NOT_FOUND)',
}

const exe = (name) => (process.platform === 'win32' ? `${name}.exe` : name)

/**
 * Candidate locations for an executor binary, in precedence order:
 * explicit override, the packaged app's resources, then dev checkouts.
 * Mirrors p4node.cjs's agentBinary() so both find binaries the same way.
 */
// Executors installed at runtime rather than shipped (the CUDA pack): id ->
// () => path|null. Set by main, which owns where such installs live.
const installed = new Map()
function setInstalledExecutor(id, fn) { installed.set(id, fn) }

function candidates({ id, envVar, resourceDir, devPaths }) {
  const file = exe(resourceDir.bin)
  const out = []
  const explicit = process.env[envVar]
  if (explicit) out.push(explicit)
  if (process.resourcesPath) out.push(path.join(process.resourcesPath, resourceDir.dir, file))
  const fromInstall = installed.has(id) ? installed.get(id)() : null
  if (fromInstall) out.push(fromInstall)
  out.push(path.join(__dirname, '..', 'resources', resourceDir.dir, process.platform, file))
  for (const p of devPaths(file)) out.push(p)
  return out
}

const REPO = path.resolve(__dirname, '..', '..', '..')

const KNOWN = [
  {
    id: 'p4-agent',
    purpose: 'Pipeline stages for the p4 engine (hosts layers an operator plan assigns).',
    locate: () => candidates({
      envVar: 'KVASIR_P4_AGENT',
      resourceDir: { dir: 'p4', bin: 'p4-agent' },
      devPaths: (file) => [path.join(REPO, '..', 'p4', 'target', 'release', file)],
    }),
    probeArgs: ['--version'],
  },
  {
    // Built from apps/linkcpp-expert-worker in this repository. (The copy on
    // the MI250 host is named linker-expert-worker; same program, different
    // build tree — the name here is what this repo's CMake target produces.)
    // On Windows it is the CUDA build (MSVC + CUDA 12.x) from the on-demand
    // pack (cudaPack.cjs). It needs only the NVIDIA driver; a machine without
    // one fails to load nvcuda.dll, which probe() reports as a missing DLL.
    id: 'linkcpp-expert-worker',
    platforms: ['win32', 'linux'],
    purpose: 'MoE expert FFN via ggml mul_mat_id — the executor a remote expert shard needs.',
    locate: () => candidates({
      id: 'linkcpp-expert-worker',
      envVar: 'KVASIR_EXPERT_WORKER',
      resourceDir: { dir: 'expert-worker', bin: 'linkcpp-expert-worker' },
      devPaths: (file) => [
        path.join(REPO, 'build', 'apps', 'linkcpp-expert-worker', file),
        path.join(REPO, 'build', 'bin', file),
      ],
    }),
    // It has --serve and --self-test; --help is the cheapest thing that makes
    // the OS load the binary and its DLLs without opening a port or a model.
    probeArgs: ['--help'],
    // What hosting N experts costs on the GPU, as N = (budget - F - S - H) / R.
    // Measured on an RTX 4060 (CUDA 12.9, sm_89 build, 2026-09-22) by loading
    // 2, 8 and 64 experts and then serving batches of 1, 64 and 512 tokens x 8
    // experts: +110 / +167 / +675 MiB after load. R ~ 9.11 MiB (the served
    // bytes, 9.06 MiB, plus allocator rounding), F ~ 92 MiB (CUDA context and
    // ggml buffers). Scratch depends on the kernels: the MMQ build took +28 MiB
    // at 512 tokens, the cuBLAS + FP32 build that ships takes +107 MiB (its
    // workspace and FP32 activations). Values below round those up.
    // This is ggml's model: it computes on the quantized bytes as served. An
    // executor that expands weights (e.g. to fp16) must report its own R.
    memoryModel: {
      residentBytesPerExpert: 9_568_256,   // 9.125 MiB
      fixedBytes: 128 * MIB,
      scratchBytes: 160 * MIB,             // cuBLAS + FP32, up to 512 tokens x 8 experts per request
      headroomBytes: 128 * MIB,
    },
  },
  {
    // The same program as above, built for Metal and shipped inside the macOS
    // app rather than downloaded: it is 78 KB against the CUDA build's 190 MB,
    // because Metal needs no redistributable runtime.
    //
    // It matters that this is the same source. Metal has k-quant kernels, so it
    // computes on the served Q4_K/Q5_K bytes unchanged — no dequantisation to
    // fp16, which would have tripled what one expert costs in memory and cut
    // what a Mac can hold from about seven layers to two.
    //
    // Checked on an M4 Pro against a float64 reference over real gateway
    // shards: cosine 1.000000 at one token, 0.999963 at 256.
    id: 'linkcpp-expert-worker-metal',
    platforms: ['darwin'],
    purpose: 'MoE expert FFN on Apple Silicon (Metal) — the macOS expert executor.',
    locate: () => candidates({
      id: 'linkcpp-expert-worker-metal',
      envVar: 'KVASIR_EXPERT_WORKER_METAL',
      resourceDir: { dir: 'expert-worker', bin: 'linkcpp-expert-worker' },
      devPaths: (file) => [
        path.join(REPO, 'build-mac-metal', 'apps', 'linkcpp-expert-worker', file),
      ],
    }),
    probeArgs: ['--help'],
    // Unified memory, so these are shares of system RAM rather than of a card.
    // R is the served size plus allocator rounding, as on CUDA: Metal holds the
    // same quantized bytes. F is smaller — no CUDA context — but the headroom
    // is larger, because over-committing here slows the whole machine and not
    // just this process.
    memoryModel: {
      residentBytesPerExpert: 9_568_256,
      fixedBytes: 64 * MIB,
      scratchBytes: 64 * MIB,
      headroomBytes: 256 * MIB,
    },
  },
]

function findBinary(paths) {
  for (const p of paths) {
    try { if (p && fs.statSync(p).isFile()) return p } catch { /* next */ }
  }
  return null
}

/**
 * Start the binary and see whether the OS loads it. Exit status does not
 * matter much — an unknown flag exiting non-zero still proves the image and
 * its DLLs loaded. What matters is a spawn error or a loader NTSTATUS.
 */
function probe(bin, args) {
  return new Promise((resolve) => {
    let child
    let settled = false
    let output = ''
    const done = (result) => { if (!settled) { settled = true; resolve(result) } }
    try {
      child = spawn(bin, args, { windowsHide: true })
    } catch (e) {
      return done({ runnable: false, reason: `could not start: ${e.message}` })
    }
    const timer = setTimeout(() => {
      // Still running after the timeout means it loaded; it just does not exit
      // on this flag. That is a pass, not a failure.
      try { child.kill() } catch { /* already gone */ }
      done({ runnable: true, reason: 'started (did not exit on the probe flag)' })
    }, PROBE_TIMEOUT_MS)
    const keep = (d) => { if (output.length < 2000) output += String(d) }
    child.stdout?.on('data', keep)
    child.stderr?.on('data', keep)
    child.on('error', (e) => {
      clearTimeout(timer)
      done({ runnable: false, reason: `could not start: ${e.code || e.message}` })
    })
    child.on('exit', (code) => {
      clearTimeout(timer)
      if (code != null && NTSTATUS[code >>> 0]) {
        return done({ runnable: false, reason: NTSTATUS[code >>> 0], exitCode: code >>> 0 })
      }
      const firstLine = output.trim().split(/\r?\n/)[0] || ''
      done({ runnable: true, reason: `loaded (exit ${code})`, banner: firstLine.slice(0, 120) })
    })
  })
}

const run = (cmd, args) => new Promise((resolve) => {
  execFile(cmd, args, { timeout: 4000, windowsHide: true }, (err, stdout) => resolve(err ? null : String(stdout)))
})

/** Live GPU state. Not cached: free VRAM moves as other local services load models. */
async function gpuReadiness() {
  // Apple Silicon has no nvidia-smi and no separate VRAM. The GPU shares the
  // machine's memory, and Metal publishes a recommended working set rather than
  // a card size — ask for more than that and the whole system starts swapping,
  // which is not how a discrete card fails. So it is reported as a vendor of
  // its own, with the recommended set as the total, and the UI is expected to
  // call it shared memory rather than VRAM.
  if (process.platform === 'darwin' && process.arch === 'arm64') {
    const chip = (await run('sysctl', ['-n', 'machdep.cpu.brand_string']) || 'Apple Silicon').trim();
    const ram = Number((await run('sysctl', ['-n', 'hw.memsize']) || '0').trim()) || 0;
    // What Metal will recommend is ~75% of physical memory on these parts. It
    // is readable exactly through MTLDevice, but that needs a native call this
    // process cannot make; the fraction is stable enough to size a slider, and
    // the executor refuses anything it cannot actually allocate.
    const working = Math.floor(ram * 0.75);
    return {
      vendor: 'apple',
      ready: ram > 0,
      unifiedMemory: true,
      gpus: ram ? [{
        name: chip,
        driver: null,
        totalBytes: working,
        usedBytes: 0,
        // Unified memory is shared with everything else running, so "free" is
        // not knowable the way it is on a card. The budget is the operator's
        // choice, bounded by the recommended working set.
        freeBytes: working,
        utilizationPct: null,
      }] : [],
    };
  }
  const out = await run('nvidia-smi', [
    '--query-gpu=name,driver_version,memory.total,memory.used,memory.free,utilization.gpu',
    '--format=csv,noheader,nounits',
  ])
  if (!out) return { vendor: null, ready: false, reason: 'no NVIDIA driver responding (nvidia-smi unavailable)' }
  const banner = await run('nvidia-smi', [])
  const cuda = banner && (banner.match(/CUDA Version:\s*([0-9.]+)/) || [])[1]
  const gpus = out.trim().split('\n').filter(Boolean).map((line) => {
    const [name, driver, total, used, free, util] = line.split(',').map((s) => s.trim())
    return {
      name,
      driver,
      totalBytes: Number(total) * MIB,
      usedBytes: Number(used) * MIB,
      freeBytes: Number(free) * MIB,
      utilizationPct: Number(util),
    }
  })
  return { vendor: 'nvidia', ready: gpus.length > 0, cudaVersion: cuda || null, gpus }
}

/**
 * @returns {Promise<{executors: object[], gpu: object, summary: object}>}
 */
async function executors() {
  const results = []
  for (const k of KNOWN) {
    // An executor that names its platforms is only looked for there. Without
    // this the Metal build is reported "not installed" on Windows and the CUDA
    // build on a Mac, which reads as something broken rather than something
    // that was never meant to be there.
    if (k.platforms && !k.platforms.includes(process.platform)) continue
    const searched = k.locate()
    const bin = findBinary(searched)
    if (!bin) {
      results.push({ id: k.id, purpose: k.purpose, found: false, runnable: false, path: null,
        reason: 'not installed', searched })
      continue
    }
    const p = await probe(bin, k.probeArgs)
    results.push({ id: k.id, purpose: k.purpose, found: true, path: bin, ...p })
  }
  const gpu = await gpuReadiness()
  // Whichever expert executor this platform ships. They are alternatives, not
  // a set: prefer one that actually runs, so a machine that has both a stale
  // copy and a working one is not judged by the stale one.
  const experts = results.filter((r) => r.id.startsWith('linkcpp-expert-worker'))
  const expert = experts.find((r) => r.runnable) ?? experts[0]
  const agent = results.find((r) => r.id === 'p4-agent')
  return {
    executors: results,
    gpu,
    // The two questions the node screen actually needs answered.
    summary: {
      canHostPipelineStages: Boolean(agent && agent.runnable),
      canServeExperts: Boolean(expert && expert.runnable && gpu.ready),
      blockers: [
        ...results.filter((r) => !r.runnable).map((r) => `${r.id}: ${r.reason}`),
        ...(gpu.ready ? [] : [`gpu: ${gpu.reason}`]),
      ],
    },
  }
}

const MAX_EXPERTS_PER_REQUEST = 64   // the bridge refuses more in one shard

/**
 * Experts that fit: floor((min(budget, available) - F - S - H) / R), clamped to
 * [0, 64]. No budget or no model yet -> the most one shard can carry, never
 * "unlimited". src/api.ts mirrors this for the slider's preview.
 */
function expertsForBudget(budget, model, availableBytes) {
  if (budget == null || !model) return MAX_EXPERTS_PER_REQUEST
  const usable = availableBytes == null ? budget : Math.min(budget, availableBytes)
  const n = Math.floor((usable - model.fixedBytes - model.scratchBytes - model.headroomBytes)
    / model.residentBytesPerExpert)
  return Math.max(0, Math.min(n, MAX_EXPERTS_PER_REQUEST))
}

// ---- holding more than one shard's worth --------------------------------------
//
// One worker process serves one GGUF of one layer (--layer is fixed at load and
// the dispatch wire has no layer field), and one shard carries at most 64
// experts. So a machine that can hold more than 64 does it as several slots:
// separate worker processes, each with its own worker id and relay session.
// Every slot pays its own fixed cost (F + S — a CUDA context each), while the
// headroom H is kept once for the machine.

// Slots per machine. The bridge has one relay listen port per worker id and a
// span of 60 for the whole fleet, so a single desktop taking dozens would
// leave other devices unable to open a relay. Raise together with that span.
const MAX_SLOTS = 8

/** Bytes one slot of `experts` experts costs, headroom excluded. */
function slotBytes(model, experts) {
  return model.fixedBytes + model.scratchBytes + experts * model.residentBytesPerExpert
}

/**
 * What the machine can hold in total: slots of up to 64 experts, packed until
 * the budget runs out. This — not one shard's window — is what the slider
 * shows. No budget or model yet: one shard's worth, never "unlimited".
 */
function capacityForBudget(budget, model, availableBytes, { maxSlots = MAX_SLOTS } = {}) {
  if (budget == null || !model) return { experts: MAX_EXPERTS_PER_REQUEST, slots: 1 }
  let left = (availableBytes == null ? budget : Math.min(budget, availableBytes)) - model.headroomBytes
  let experts = 0
  let slots = 0
  while (slots < maxSlots) {
    const n = Math.min(MAX_EXPERTS_PER_REQUEST, Math.floor((left - model.fixedBytes - model.scratchBytes) / model.residentBytesPerExpert))
    if (n < 1) break
    experts += n
    slots += 1
    left -= slotBytes(model, n)
  }
  return { experts, slots }
}

/**
 * The window to ask for next, given the slots already held (their expert
 * counts): what still fits after them, as one more slot, clamped to 64.
 * 0 means "do not volunteer". Unknown budget or model: one slot of 64 at most.
 */
function nextSlotWindow(budget, model, availableBytes, heldCounts, { maxSlots = MAX_SLOTS } = {}) {
  if (heldCounts.length >= maxSlots) return 0
  if (budget == null || !model) return heldCounts.length ? 0 : MAX_EXPERTS_PER_REQUEST
  const usable = (availableBytes == null ? budget : Math.min(budget, availableBytes)) - model.headroomBytes
  const used = heldCounts.reduce((s, n) => s + slotBytes(model, n), 0)
  const n = Math.floor((usable - used - model.fixedBytes - model.scratchBytes) / model.residentBytesPerExpert)
  return Math.max(0, Math.min(n, MAX_EXPERTS_PER_REQUEST))
}

/**
 * How many of the held slots (oldest first) still fit the budget. A lowered
 * budget releases the newest slots beyond this.
 */
function slotsThatFit(budget, model, availableBytes, heldCounts) {
  if (budget == null || !model) return heldCounts.length
  const usable = (availableBytes == null ? budget : Math.min(budget, availableBytes)) - model.headroomBytes
  let used = 0
  for (let i = 0; i < heldCounts.length; i++) {
    used += slotBytes(model, heldCounts[i])
    if (used > usable) return i
  }
  return heldCounts.length
}

/** Path of an installed executor binary, or null. Does not start it. */
function locateExecutor(id) {
  const k = KNOWN.find((x) => x.id === id)
  return k ? findBinary(k.locate()) : null
}

/**
 * The expert executor for this platform (CUDA on Windows/Linux, Metal on a
 * Mac): the KNOWN entry, and its binary if installed. Prefers one that is
 * installed, so a stale entry never shadows a working one.
 */
function expertExecutor() {
  const mine = KNOWN.filter((k) => k.id.startsWith('linkcpp-expert-worker')
    && (!k.platforms || k.platforms.includes(process.platform)))
  for (const k of mine) {
    const bin = findBinary(k.locate())
    if (bin) return { entry: k, path: bin }
  }
  return { entry: mine[0] || null, path: null }
}

module.exports = {
  executors, expertsForBudget, capacityForBudget, nextSlotWindow, slotsThatFit, slotBytes,
  expertExecutor, locateExecutor, setInstalledExecutor, gpuReadiness,
  KNOWN, NTSTATUS, MAX_EXPERTS_PER_REQUEST, MAX_SLOTS,
}
