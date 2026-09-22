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
 * ## No NVIDIA DLLs in the pack
 *
 * The worker runs the expert FFN on Q4_K/Q5_K weights with mul_mat_id. With
 * GGML_CUDA_FORCE_MMQ those go through ggml's own quantized kernels, never
 * cuBLAS — measured: identical outputs with and without cuBLAS present, up to
 * 512 tokens x 8 experts per request. ggml-cuda still links cuBLAS, so it is
 * delay-loaded: the DLL is only looked for if a cuBLAS call is ever made. That
 * takes ~770 MB of NVIDIA DLLs out of the pack. cudart is linked statically.
 *
 * MMQ needs compute capability >= 6.1 (DP4A); below that ggml would fall back
 * to cuBLAS. So the arch list starts at 6.1 and the app refuses older GPUs
 * before downloading anything (PACKS.win32.minComputeCapability).
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
    '-DGGML_CUDA=ON', '-DGGML_CUDA_FORCE_MMQ=ON', '-DGGML_STATIC=ON', '-DGGML_OPENMP=OFF',
    '-DCMAKE_MSVC_RUNTIME_LIBRARY=MultiThreaded', `"-DCMAKE_CUDA_ARCHITECTURES=${ARCHS}"`,
    '"-DCMAKE_EXE_LINKER_FLAGS=/DELAYLOAD:cublas64_12.dll delayimp.lib"',
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

function pack({ exe, cuda }) {
  const rev = execFileSync('git', ['-C', REPO, 'rev-parse', '--short=8', 'HEAD'], { encoding: 'utf8' }).trim()
  const now = new Date()
  const date = [now.getFullYear(), now.getMonth() + 1, now.getDate()].map((n) => String(n).padStart(2, '0')).join('.')
  const version = `${date}-${rev}`
  const stage = path.join(REPO, 'build', `expert-pack-${version}`)
  fs.rmSync(stage, { recursive: true, force: true })
  fs.mkdirSync(stage, { recursive: true })
  fs.copyFileSync(exe, path.join(stage, EXE))
  // cudart is linked in statically, so its license notice travels with it.
  const license = path.join(cuda, 'LICENSE')
  if (fs.existsSync(license)) fs.copyFileSync(license, path.join(stage, 'NVIDIA-CUDA-LICENSE.txt'))
  fs.writeFileSync(path.join(stage, 'README.txt'), [
    'Kvasir expert worker - Windows x64, NVIDIA CUDA',
    `version ${version}`,
    '',
    `${EXE}  built from apps/linkcpp-expert-worker at ${rev}`,
    `  MSVC, static CRT, CUDA runtime linked statically, archs ${ARCHS}`,
    '  ggml quantized kernels only (MMQ); cuBLAS is not used or shipped',
    '',
    'Requires an NVIDIA GPU with compute capability 6.1 or newer and a driver',
    'supporting CUDA 12.x. The CUDA runtime is distributed under the terms in',
    'NVIDIA-CUDA-LICENSE.txt.',
    '',
  ].join('\r\n'))
  const zip = path.join(REPO, 'build', `kvasir-expert-worker-win-x64-cuda12-${version}.zip`)
  fs.rmSync(zip, { force: true })
  // Windows 10+ ships bsdtar, which writes zip with -a. Named by full path:
  // a Git-for-Windows tar earlier on PATH reads "C:\\..." as a remote host.
  const tar = path.join(process.env.SystemRoot || 'C:\\Windows', 'System32', 'tar.exe')
  execFileSync(tar, ['-a', '-c', '-f', zip, '-C', stage, '.'], { stdio: 'inherit' })
  const sha256 = crypto.createHash('sha256').update(fs.readFileSync(zip)).digest('hex')
  const bytes = fs.statSync(zip).size
  console.log(`\npack: ${zip}\n  version ${version}\n  bytes   ${bytes}\n  sha256  ${sha256}`)
  console.log('\nPin these in electron/cudaPack.cjs PACKS.win32 once the file is hosted.')
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
