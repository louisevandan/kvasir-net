#!/usr/bin/env node
/**
 * Build the p4 agents this app ships, and put them where the packer expects.
 *
 * The app shipped no agent at all until now. `agentBinary()` looked in three
 * places, found nothing on any machine where somebody had not built p4 by hand,
 * and so the node button could not work — on every machine we would ever hand
 * this to. The relay made a node reachable; this gives it something to run.
 *
 * ## One directory per platform, because the packer takes the lot
 *
 *   resources/p4/darwin/p4-agent       universal: arm64 + x86_64
 *   resources/p4/win32/p4-agent.exe
 *   resources/p4/linux/p4-agent
 *
 * electron-builder copies a resource directory wholesale, so each platform gets
 * its own and no build carries another platform's binary.
 *
 * ## The universal slice is not optional on macOS
 *
 * The Mac app is packed as `universal` and extraResources are copied into both
 * slices unchanged. An arm64-only binary gives an Intel Mac an app that looks
 * complete and cannot start a node — a failure that only shows up on hardware
 * the developer does not own. This builds both and `lipo`s them, and fails
 * loudly rather than shipping one architecture quietly.
 *
 * ## Windows: MSVC natively, mingw when cross-compiling
 *
 * On Windows this builds `x86_64-pc-windows-msvc` with the C runtime linked
 * statically, so the agent needs nothing beyond system DLLs (needs Visual
 * Studio Build Tools with the C++ workload). From another OS it cross-compiles
 * `x86_64-pc-windows-gnu`, which needs that Rust target and a mingw-w64 linker
 * (`brew install mingw-w64`). Without them this says so and stops, rather than
 * producing a Windows package with no agent in it.
 *
 * ## Where p4 comes from
 *
 * A separate repository. Point `KVASIR_P4_SRC` at a checkout or leave it beside
 * this one. It is never vendored here: a compiled binary in git is something
 * nobody reviews and everybody forgets to rebuild.
 *
 * Usage:
 *   node scripts/build-agent.cjs                 this platform
 *   node scripts/build-agent.cjs --platform win32
 *   node scripts/build-agent.cjs --all           everything this machine can
 */
const { execFileSync } = require('node:child_process')
const fs = require('node:fs')
const path = require('node:path')

const HERE = __dirname
const ROOT = path.join(HERE, '..', 'resources', 'p4')

/** What each platform needs built, and what the result is called. */
const PLATFORMS = {
  darwin: { targets: ['aarch64-apple-darwin', 'x86_64-apple-darwin'], exe: 'p4-agent', merge: true },
  win32: {
    targets: [process.platform === 'win32' ? 'x86_64-pc-windows-msvc' : 'x86_64-pc-windows-gnu'],
    exe: 'p4-agent.exe',
    merge: false,
  },
  linux: { targets: ['x86_64-unknown-linux-gnu'], exe: 'p4-agent', merge: false },
}

function p4Source() {
  const told = process.env.KVASIR_P4_SRC
  if (told) {
    if (!fs.existsSync(path.join(told, 'Cargo.toml'))) throw new Error(`KVASIR_P4_SRC=${told} has no Cargo.toml`)
    return told
  }
  const repo = path.resolve(HERE, '..', '..', '..')
  for (const guess of [path.join(repo, '..', 'p4'), path.join(repo, 'p4')]) {
    if (fs.existsSync(path.join(guess, 'Cargo.toml'))) return guess
  }
  throw new Error('no p4 checkout found. Clone louisevandan/p4 beside this repository, or set KVASIR_P4_SRC.')
}

const run = (cmd, args, opts = {}) => execFileSync(cmd, args, { stdio: 'inherit', ...opts })
// Probing a file's architecture is allowed to fail — `lipo` has nothing to say
// about a PE binary — so its complaint must not reach the build log as if
// something had gone wrong.
const quiet = (cmd, args) => {
  try { return execFileSync(cmd, args, { encoding: 'utf8', stdio: ['ignore', 'pipe', 'ignore'] }).trim() }
  catch { return '' }
}

function buildTarget(source, target, exe) {
  console.log(`\n=== ${target}`)
  try {
    // A static CRT keeps the MSVC build free of the Visual C++ redistributable.
    const env = target.endsWith('-msvc')
      ? { ...process.env, RUSTFLAGS: `${process.env.RUSTFLAGS || ''} -C target-feature=+crt-static`.trim() }
      : process.env
    run('cargo', ['build', '--release', '-p', 'p4-agent', '--bin', 'p4-agent', '--target', target], { cwd: source, env })
  } catch (error) {
    const installed = quiet('rustup', ['target', 'list', '--installed'])
    const hint = installed.includes(target)
      ? (target.includes('windows-gnu')
          ? 'the target is installed, so this is the linker: brew install mingw-w64'
          : 'the target is installed, so check the linker for it')
      : `install it first: rustup target add ${target}`
    throw new Error(`cargo could not build ${target}. ${hint}`)
  }
  const built = path.join(source, 'target', target, 'release', exe)
  if (!fs.existsSync(built)) throw new Error(`cargo reported success but ${built} is missing`)
  return built
}

function buildPlatform(source, platform) {
  const spec = PLATFORMS[platform]
  if (!spec) throw new Error(`unknown platform ${platform}`)
  const outDir = path.join(ROOT, platform)
  fs.mkdirSync(outDir, { recursive: true })
  const destination = path.join(outDir, spec.exe)

  const slices = spec.targets.map((target) => buildTarget(source, target, spec.exe))
  if (spec.merge && slices.length > 1) {
    console.log(`\n=== merging ${slices.length} slices`)
    run('lipo', ['-create', ...slices, '-output', destination])
  } else {
    fs.copyFileSync(slices[0], destination)
  }
  fs.chmodSync(destination, 0o755)

  const size = (fs.statSync(destination).size / 1e6).toFixed(1)
  const arches = quiet('lipo', ['-archs', destination]) || quiet('file', ['-b', destination]).slice(0, 70)
  console.log(`\n${platform}: ${path.relative(process.cwd(), destination)}  ${size} MB · ${arches}`)
  if (platform === 'darwin' && !arches.includes('x86_64')) {
    throw new Error('the merged binary has no x86_64 slice — an Intel Mac would ship without an agent')
  }
  return destination
}

function main() {
  const argv = process.argv.slice(2)
  const source = p4Source()
  console.log(`p4 source: ${source}`)

  let wanted
  if (argv.includes('--all')) wanted = Object.keys(PLATFORMS)
  else {
    const at = argv.indexOf('--platform')
    wanted = at > -1 && argv[at + 1] ? [argv[at + 1]] : [process.platform]
  }

  const done = []
  const failed = []
  for (const platform of wanted) {
    try { buildPlatform(source, platform); done.push(platform) }
    catch (error) {
      failed.push(`${platform}: ${error.message}`)
      // With --all, one missing cross-toolchain should not lose the builds that
      // did work. On a single platform it is fatal.
      if (wanted.length === 1) throw error
    }
  }
  console.log(`\nbuilt: ${done.join(', ') || 'nothing'}`)
  for (const line of failed) console.error(`skipped ${line}`)
  if (!done.length) process.exit(1)
}

try { main() } catch (error) { console.error(`\nbuild-agent: ${error.message}`); process.exit(1) }
