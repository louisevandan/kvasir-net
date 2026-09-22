#!/usr/bin/env node
/**
 * Build the expert worker for the platform this runs on.
 *
 * The same source, apps/linkcpp-expert-worker, but delivered two different
 * ways, because the two backends are not remotely the same size.
 *
 *   Windows (CUDA)   190 MB, so it is NOT in the installer: this zips it as
 *                    the versioned pack whose SHA-256 the app pins and fetches
 *                    on demand (electron/cudaPack.cjs).
 *   macOS (Metal)    2.2 MB, because Metal needs no redistributable runtime.
 *                    Small enough to bundle, so it goes into resources/ and
 *                    ships inside the .app — nothing to download, nothing to
 *                    pin, nothing to host.
 *
 * Neither cross-compiles: CUDA for Windows needs MSVC and nvcc on the host,
 * and Metal needs Xcode's toolchain. Each platform builds its own.
 *
 * ## cuBLAS, and why the pack is two parts
 *
 * The first pack forced MMQ and left cuBLAS out. That was measured wrong: the
 * check compared MMQ with MMQ. Against a float64 oracle from the shard's own
 * weights, MMQ falls under cosine 0.999 as soon as a dispatch is wider than 8
 * tokens (8 of 256 tokens at T=256 on an RTX 4060; the same on GB10), because
 * above MMVQ_MAX_BATCH_SIZE ggml switches to MMQ. Chunking to 8 keeps it
 * correct but costs 3-6x on prefill-width batches. cuBLAS with FP32 compute
 * passes at every width measured (T=1..512, min cosine 1.000000 as displayed)
 * and is 25-33% slower than MMQ, 2-4.6x faster than chunking. So the worker is
 * built with GGML_CUDA_FORCE_CUBLAS; it sets GGML_CUDA_FORCE_CUBLAS_COMPUTE_32F
 * itself and does not chunk (LINKCPP_CUDA_CUBLAS in main.cpp).
 *
 * That brings back cublas64_12 + cublasLt64_12, ~770 MB. They change only
 * with the CUDA release, while the worker changes with every fix, so they ship
 * as a separate part: runtime (the DLLs, named by their content hash, fetched
 * once) and worker (the exe). A worker update is then only the worker.
 * cudart is linked statically. The arch list starts at 6.1, as before.
 *
 * Known and not yet fixed: the exe embeds this machine's source paths (ggml's
 * assert messages carry __FILE__), so a shipped pack names the build account
 * and checkout layout. It is not a runtime dependency. Mapping them away needs
 * a prefix map on both compilers — /pathmap for MSVC, -Xcompiler for nvcc's
 * host pass — which is the next change to this build.
 *
 * Needs: Visual Studio Build Tools (C++), and a CUDA 12.x toolkit — the
 * installer, or NVIDIA's redist zips unpacked into one tree — at CUDA_PATH or
 * KVASIR_CUDA_ROOT.
 *
 * Usage:
 *   node scripts/build-expert-worker.cjs            build (Windows: + pack)
 *   node scripts/build-expert-worker.cjs --no-pack  build only; skip the Windows pack
 */
const { execFileSync } = require('node:child_process')
const crypto = require('node:crypto')
const fs = require('node:fs')
const path = require('node:path')

const HERE = __dirname
const REPO = path.resolve(HERE, '..', '..', '..')
const BUILD = path.join(REPO, 'build', 'win-cuda-pack')
const METAL_BUILD = path.join(REPO, 'build-mac-metal')
const EXE = 'linkcpp-expert-worker.exe'
const MACH_O = 'linkcpp-expert-worker'
const ARCHS = '61-virtual;70-virtual;75-virtual;80-virtual;86-real;89-real;90-virtual;120a-real;121a-real'

function vcvars() {
  const vswhere = path.join(process.env['ProgramFiles(x86)'] || 'C:\\Program Files (x86)', 'Microsoft Visual Studio', 'Installer', 'vswhere.exe')
  const root = execFileSync(vswhere, ['-latest', '-products', '*', '-requires', 'Microsoft.VisualStudio.Component.VC.Tools.x86.x64', '-property', 'installationPath'], { encoding: 'utf8' }).trim()
  if (!root) throw new Error('no Visual Studio with the C++ tools found')
  return { bat: path.join(root, 'VC', 'Auxiliary', 'Build', 'vcvars64.bat'), cmakeDir: path.join(root, 'Common7', 'IDE', 'CommonExtensions', 'Microsoft', 'CMake') }
}

function cudaRoot() {
  const root = process.env.KVASIR_CUDA_ROOT || process.env.CUDA_PATH
  if (!root || !fs.existsSync(path.join(root, 'bin', 'nvcc.exe'))) {
    throw new Error('no CUDA toolkit: set CUDA_PATH or KVASIR_CUDA_ROOT to a tree with bin/nvcc.exe')
  }
  return root
}

function build() {
  const vs = vcvars()
  const cuda = cudaRoot().replace(/\\/g, '/')
  const cmakeBin = path.join(vs.cmakeDir, 'CMake', 'bin')
  const ninja = path.join(vs.cmakeDir, 'Ninja')
  const configure = [
    'cmake', '-S', `"${REPO}"`, '-B', `"${BUILD}"`, '-G', 'Ninja', '-DCMAKE_BUILD_TYPE=Release',
    '-DLINKCPP_EXPERT_WORKER_ONLY=ON', '-DGGML_NATIVE=OFF', '-DBUILD_SHARED_LIBS=OFF',
    '-DGGML_CUDA=ON', '-DGGML_CUDA_FORCE_CUBLAS=ON', '-DGGML_STATIC=ON', '-DGGML_OPENMP=OFF',
    '-DCMAKE_MSVC_RUNTIME_LIBRARY=MultiThreaded', `"-DCMAKE_CUDA_ARCHITECTURES=${ARCHS}"`,
    `-DCUDAToolkit_ROOT=${cuda}`, `-DCMAKE_CUDA_COMPILER=${cuda}/bin/nvcc.exe`,
  ].join(' ')
  const script = [
    '@echo off',
    `call "${vs.bat}" >nul || exit /b 1`,
    `set "PATH=${cmakeBin};${ninja};${cuda.replace(/\//g, '\\')}\\bin;%PATH%"`,
    `${configure} || exit /b 1`,
    `cmake --build "${BUILD}" --target linkcpp-expert-worker || exit /b 1`,
  ].join('\r\n')
  const bat = path.join(BUILD + '.bat')
  fs.mkdirSync(path.dirname(bat), { recursive: true })
  fs.writeFileSync(bat, script)
  execFileSync('cmd', ['/c', bat], { stdio: 'inherit' })
  const exe = path.join(BUILD, 'apps', 'linkcpp-expert-worker', EXE)
  if (!fs.existsSync(exe)) throw new Error(`build reported success but ${exe} is missing`)
  return { exe, cuda }
}

/**
 * The macOS worker: one static arm64 executable against the system frameworks.
 *
 * Three flags here are load-bearing and each was chosen against an alternative
 * that also builds:
 *
 *   BUILD_SHARED_LIBS=OFF   The default leaves five libggml*.dylib beside the
 *     binary, found through @rpath. That is five more files to sign, notarise
 *     and keep in step, and a worker that dies at spawn if any is missing. The
 *     static link is 2.2 MB and self-contained, which is also the shape the
 *     Windows build already has.
 *
 *   GGML_METAL_EMBED_LIBRARY=ON   Otherwise ggml loads default.metallib from
 *     disk at runtime, relative to the executable — which is not where it ends
 *     up inside an .app bundle. Embedding it removes the question.
 *
 *   GGML_NATIVE=OFF   On by default it tunes for the build machine's CPU. An
 *     M4 build would then fault on an M1. The Metal path does the arithmetic
 *     anyway, so there is nothing to win and a whole class of Mac to lose.
 *
 * Verified after those changes against a float64 reference over real gateway
 * shards (layer 4, experts 0-2): cosine 1.000000 at one token, 0.999962 at 256
 * — unchanged from the shared-library build, as it should be.
 */
function buildMetal() {
  if (process.arch !== 'arm64') {
    throw new Error('the Metal worker is Apple Silicon only; this is ' + process.arch)
  }
  const configure = [
    '-S', REPO, '-B', METAL_BUILD, '-DCMAKE_BUILD_TYPE=Release',
    '-DLINKCPP_EXPERT_WORKER_ONLY=ON', '-DBUILD_SHARED_LIBS=OFF', '-DGGML_STATIC=ON',
    '-DGGML_METAL=ON', '-DGGML_METAL_EMBED_LIBRARY=ON',
    '-DGGML_NATIVE=OFF', '-DGGML_OPENMP=OFF',
    '-DCMAKE_OSX_ARCHITECTURES=arm64', '-DCMAKE_OSX_DEPLOYMENT_TARGET=11.0',
  ]
  execFileSync('cmake', configure, { stdio: 'inherit' })
  execFileSync('cmake', ['--build', METAL_BUILD, '--target', 'linkcpp-expert-worker',
    '-j', String(require('node:os').cpus().length)], { stdio: 'inherit' })
  const bin = path.join(METAL_BUILD, 'apps', 'linkcpp-expert-worker', MACH_O)
  if (!fs.existsSync(bin)) throw new Error(`build reported success but ${bin} is missing`)

  // A dynamic link here would mean the three flags above did not take, and the
  // failure would otherwise surface as a worker that will not spawn on someone
  // else's Mac. Cheaper to catch it now than in a shipped .app.
  const linked = execFileSync('otool', ['-L', bin], { encoding: 'utf8' })
  const foreign = linked.split('\n').slice(1)
    .map((l) => l.trim().split(' ')[0]).filter(Boolean)
    .filter((l) => !l.startsWith('/usr/lib/') && !l.startsWith('/System/'))
  if (foreign.length) throw new Error(`not self-contained, links ${foreign.join(', ')}`)

  // resources/expert-worker/<platform> is where executors.cjs looks in a dev
  // checkout, and package.json ships this directory as the .app's
  // Resources/expert-worker.
  const dest = path.join(HERE, '..', 'resources', 'expert-worker', 'darwin')
  fs.mkdirSync(dest, { recursive: true })
  fs.copyFileSync(bin, path.join(dest, MACH_O))
  fs.chmodSync(path.join(dest, MACH_O), 0o755)
  console.log(`\nbundled: ${path.join(dest, MACH_O)}  ${fs.statSync(bin).size} bytes`)
  console.log('It ships inside the app; there is no pack to publish and no hash to pin.')
}

const RUNTIME_DLLS = ['cublas64_12.dll', 'cublasLt64_12.dll']

// Windows 10+ ships bsdtar, which writes zip with -a. Named by full path:
// a Git-for-Windows tar earlier on PATH reads "C:\\..." as a remote host.
function zipDir(stage, zip) {
  fs.rmSync(zip, { force: true })
  const tar = path.join(process.env.SystemRoot || 'C:\\Windows', 'System32', 'tar.exe')
  execFileSync(tar, ['-a', '-c', '-f', zip, '-C', stage, '.'], { stdio: 'inherit' })
  return {
    bytes: fs.statSync(zip).size,
    sha256: crypto.createHash('sha256').update(fs.readFileSync(zip)).digest('hex'),
  }
}

/**
 * Two zips, each pinned on its own: the NVIDIA runtime and our worker.
 *
 * The runtime part is versioned by the DLLs' content hash, not by date or
 * commit, so rebuilding the worker against the same CUDA release produces
 * the same runtime version: the app sees it is already installed and fetches
 * only the new worker.
 */
function pack({ exe, cuda }) {
  const rev = execFileSync('git', ['-C', REPO, 'rev-parse', '--short=8', 'HEAD'], { encoding: 'utf8' }).trim()
  const now = new Date()
  const date = [now.getFullYear(), now.getMonth() + 1, now.getDate()].map((n) => String(n).padStart(2, '0')).join('.')
  const workerVersion = `${date}-${rev}`
  const out = path.join(REPO, 'build')
  const license = path.join(cuda, 'LICENSE')

  // runtime
  const h = crypto.createHash('sha256')
  for (const f of RUNTIME_DLLS) {
    const src = path.join(cuda, 'bin', f)
    if (!fs.existsSync(src)) throw new Error(`${src} missing: the CUDA tree needs libcublas`)
    h.update(fs.readFileSync(src))
  }
  const runtimeVersion = `cublas12-${h.digest('hex').slice(0, 12)}`
  const rStage = path.join(out, `expert-runtime-${runtimeVersion}`)
  fs.rmSync(rStage, { recursive: true, force: true })
  fs.mkdirSync(rStage, { recursive: true })
  for (const f of RUNTIME_DLLS) fs.copyFileSync(path.join(cuda, 'bin', f), path.join(rStage, f))
  if (fs.existsSync(license)) fs.copyFileSync(license, path.join(rStage, 'NVIDIA-CUDA-LICENSE.txt'))
  fs.writeFileSync(path.join(rStage, 'README.txt'), [
    'Kvasir expert worker - NVIDIA runtime (Windows x64)',
    `runtime ${runtimeVersion}`,
    '',
    `${RUNTIME_DLLS.join(', ')}: NVIDIA cuBLAS, redistributed unmodified`,
    'under the terms in NVIDIA-CUDA-LICENSE.txt.',
    '',
  ].join('\r\n'))
  const rZip = path.join(out, `kvasir-expert-runtime-win-x64-${runtimeVersion}.zip`)
  const r = zipDir(rStage, rZip)

  // worker
  const wStage = path.join(out, `expert-worker-${workerVersion}`)
  fs.rmSync(wStage, { recursive: true, force: true })
  fs.mkdirSync(wStage, { recursive: true })
  fs.copyFileSync(exe, path.join(wStage, EXE))
  // cudart is linked in statically, so its license notice travels with it too.
  if (fs.existsSync(license)) fs.copyFileSync(license, path.join(wStage, 'NVIDIA-CUDA-LICENSE.txt'))
  fs.writeFileSync(path.join(wStage, 'README.txt'), [
    'Kvasir expert worker - Windows x64, NVIDIA CUDA',
    `worker ${workerVersion}, needs runtime ${runtimeVersion}`,
    '',
    `${EXE}  built from apps/linkcpp-expert-worker at ${rev}`,
    `  MSVC, static CRT, CUDA runtime linked statically, archs ${ARCHS}`,
    '  cuBLAS with FP32 compute for wide batches (the DLLs are the runtime part)',
    '',
    'Requires an NVIDIA GPU with compute capability 6.1 or newer and a driver',
    'supporting CUDA 12.x.',
    '',
  ].join('\r\n'))
  const wZip = path.join(out, `kvasir-expert-worker-win-x64-cuda12-${workerVersion}.zip`)
  const w = zipDir(wStage, wZip)

  const pin = {
    version: workerVersion,
    parts: [
      { id: 'runtime', version: runtimeVersion, file: path.basename(rZip), sha256: r.sha256, bytes: r.bytes, files: RUNTIME_DLLS },
      { id: 'worker', version: workerVersion, file: path.basename(wZip), sha256: w.sha256, bytes: w.bytes, files: [EXE] },
    ],
  }
  console.log(`\nruntime: ${rZip}\n  ${r.bytes} bytes  sha256 ${r.sha256}`)
  console.log(`worker:  ${wZip}\n  ${w.bytes} bytes  sha256 ${w.sha256}`)
  console.log('\nPin in electron/cudaPack.cjs PACKS.win32 (add each part\'s url once hosted):')
  console.log(JSON.stringify(pin, null, 2))
}

function main() {
  if (process.platform === 'darwin') return buildMetal()
  if (process.platform !== 'win32') {
    throw new Error(`no expert-worker build is defined for ${process.platform}`)
  }
  const built = build()
  const dev = path.join(HERE, '..', 'resources', 'expert-worker', 'win32')
  fs.mkdirSync(dev, { recursive: true })
  try {
    fs.copyFileSync(built.exe, path.join(dev, EXE))
    console.log(`dev copy: ${path.join(dev, EXE)}`)
  } catch (e) {
    // A running dev app's worker holds the file open on Windows. The pack is
    // what matters; the dev copy can be refreshed after the app stops.
    console.warn(`dev copy skipped (${e.code || e.message}) — is a worker from it still running?`)
  }
  if (!process.argv.includes('--no-pack')) pack(built)
}

try { main() } catch (e) { console.error(`\nbuild-expert-worker: ${e.message}`); process.exit(1) }
