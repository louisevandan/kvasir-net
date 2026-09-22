#!/usr/bin/env node
/**
 * Build the headless Linux node release, and the index an installer reads.
 *
 * A server operator should not need a checkout, a toolchain, or npm to run a
 * node. What they need is this tarball, a Node runtime, and a worker binary —
 * and install.sh gets all three. This makes the first one.
 *
 * ## What travels and what does not
 *
 * The node itself is tiny: kvasir-node.cjs, the three modules it shares with
 * the desktop app, and two dependencies (ws, tweetnacl) that come to 392 KB.
 * p4-agent is 8.8 MB and joins it, because a server that can serve experts can
 * also serve layers and fetching it later would be a second download for no
 * reason.
 *
 * The expert worker does NOT travel. It is specific to the GPU vendor and the
 * instruction set, and on Linux it needs cuBLAS besides — ggml links it
 * unconditionally there, with no delay-load escape. Carrying every
 * architecture would turn a 9 MB install into hundreds of megabytes for a
 * machine that will use one of them. So `kvasir-node worker --install` fetches
 * the right one afterwards, the way the desktop app fetches its CUDA pack.
 *
 * ## The three modules
 *
 * participation.cjs, expertHost.cjs and executors.cjs are copied here rather
 * than duplicated in the repository, so a server and a desktop run the same
 * market, shard and worker logic. That is also why they must stay free of
 * Electron: the moment one of them requires it, this package stops working and
 * nothing in the desktop build would notice.
 *
 * Usage:
 *   node scripts/package-node-linux.cjs --arch x64 [--p4 <path to p4-agent>]
 *   node scripts/package-node-linux.cjs --index            rewrite latest.json
 */
const { execFileSync } = require('node:child_process')
const crypto = require('node:crypto')
const fs = require('node:fs')
const path = require('node:path')

const HERE = __dirname
const REPO = path.resolve(HERE, '..')
const CLI = path.join(REPO, 'node-cli')
const DESKTOP = path.join(REPO, 'wallet', 'desktop', 'electron')
const OUT = path.join(REPO, 'build', 'node-release')

/** The three files a server shares with the desktop app. */
const SHARED = ['participation.cjs', 'expertHost.cjs', 'executors.cjs']
/** Runtime dependencies. Deliberately short; anything longer deserves a look. */
const DEPS = ['ws', 'tweetnacl']

const sha256 = (file) => crypto.createHash('sha256').update(fs.readFileSync(file)).digest('hex')

function version() {
  const rev = execFileSync('git', ['-C', REPO, 'rev-parse', '--short=8', 'HEAD'], { encoding: 'utf8' }).trim()
  const now = new Date()
  const date = [now.getFullYear(), now.getMonth() + 1, now.getDate()]
    .map((n) => String(n).padStart(2, '0')).join('.')
  return `${date}-${rev}`
}

/** Copy a tree, refusing anything that is not a file or directory. */
function copyTree(from, to) {
  fs.mkdirSync(to, { recursive: true })
  for (const entry of fs.readdirSync(from, { withFileTypes: true })) {
    const src = path.join(from, entry.name)
    const dst = path.join(to, entry.name)
    if (entry.isDirectory()) copyTree(src, dst)
    else if (entry.isFile()) fs.copyFileSync(src, dst)
    // Symlinks and the rest are skipped: a release should not carry a link
    // into a path that will not exist on the machine that unpacks it.
  }
}

function build({ arch, p4 }) {
  const v = version()
  const stage = path.join(OUT, `kvasir-node-${v}`)
  fs.rmSync(stage, { recursive: true, force: true })
  fs.mkdirSync(stage, { recursive: true })

  fs.copyFileSync(path.join(CLI, 'kvasir-node.cjs'), path.join(stage, 'kvasir-node.cjs'))
  fs.chmodSync(path.join(stage, 'kvasir-node.cjs'), 0o755)
  fs.copyFileSync(path.join(CLI, 'kvasir-node.service'), path.join(stage, 'kvasir-node.service'))
  fs.copyFileSync(path.join(CLI, 'install.sh'), path.join(stage, 'install.sh'))
  fs.chmodSync(path.join(stage, 'install.sh'), 0o755)

  fs.mkdirSync(path.join(stage, 'lib'))
  for (const name of SHARED) {
    const src = path.join(DESKTOP, name)
    const text = fs.readFileSync(src, 'utf8')
    // The invariant this package depends on, checked rather than trusted: a
    // require('electron') here would only fail on someone's server, hours
    // after the build that introduced it.
    if (/require\(\s*['"]electron['"]/.test(text)) {
      throw new Error(`${name} requires electron — it cannot be shared with a headless node`)
    }
    fs.copyFileSync(src, path.join(stage, 'lib', name))
  }

  for (const dep of DEPS) {
    const from = path.join(CLI, 'node_modules', dep)
    if (!fs.existsSync(from)) throw new Error(`${dep} is not installed; run npm install in node-cli`)
    copyTree(from, path.join(stage, 'node_modules', dep))
  }
  fs.writeFileSync(path.join(stage, 'package.json'), `${JSON.stringify({
    name: 'kvasir-node', version: v, private: true, main: 'kvasir-node.cjs',
  }, null, 2)}\n`)

  if (p4) {
    fs.mkdirSync(path.join(stage, 'p4'))
    fs.copyFileSync(p4, path.join(stage, 'p4', 'p4-agent'))
    fs.chmodSync(path.join(stage, 'p4', 'p4-agent'), 0o755)
  }

  fs.writeFileSync(path.join(stage, 'VERSION'), `${v}\n`)
  fs.writeFileSync(path.join(stage, 'README.txt'), readme(v, arch, Boolean(p4)))

  const tarball = path.join(OUT, `kvasir-node-linux-${arch}-${v}.tar.gz`)
  fs.rmSync(tarball, { force: true })
  // --numeric-owner so the archive carries no account from this machine.
  //
  // --no-xattrs and COPYFILE_DISABLE matter more than they look: macOS tags
  // every file it touches with com.apple.provenance, bsdtar stores those as
  // extended headers, and GNU tar on the server then prints a warning per file
  // while unpacking. Eleven lines of "Ignoring unknown extended header
  // keyword" is the first thing an operator sees of this project.
  execFileSync('tar', [
    '--numeric-owner', '--uid=0', '--gid=0', '--no-xattrs', '--no-mac-metadata',
    '-czf', tarball, '-C', OUT, path.basename(stage),
  ], { stdio: 'inherit', env: { ...process.env, COPYFILE_DISABLE: '1' } })

  const digest = sha256(tarball)
  console.log(`\n${tarball}`)
  console.log(`  version  ${v}`)
  console.log(`  bytes    ${fs.statSync(tarball).size}`)
  console.log(`  sha256   ${digest}`)
  return { version: v, arch, file: path.basename(tarball), sha256: digest }
}

function readme(v, arch, hasP4) {
  return [
    'Kvasir node — headless Linux',
    `version ${v}  ·  linux-${arch}`,
    '',
    'A node lends GPU memory to host expert shards for the Kvasir network and',
    'earns KVR for the work it carries. It needs no stake and no balance: the',
    'wallet it earns into can be empty, and settlement fees are the network\'s.',
    '',
    'Contents',
    '  kvasir-node          a launcher that pins the Node runtime install.sh chose',
    '  kvasir-node.cjs      the node itself',
    '  lib/                 the market, shard and worker logic, shared verbatim',
    '                       with the desktop app',
    '  node_modules/        ws and tweetnacl, nothing else',
    hasP4 ? '  p4/p4-agent          the layer-serving agent (a second, separate role)' : '',
    '  kvasir-node.service  a systemd unit, with the budget left for you to set',
    '',
    'Requirements',
    '  Node.js 20 or newer (install.sh fetches one privately if yours is older)',
    '  An NVIDIA GPU, its driver, and the CUDA 13 runtime libraries',
    '  glibc 2.39 or newer for p4-agent (Ubuntu 24.04 and later)',
    '',
    'The expert worker is not in here. It is specific to your GPU and needs',
    'cuBLAS, so it is fetched separately:',
    '',
    '  ./kvasir-node worker --install',
    '',
    'Getting started',
    '  ./kvasir-node keygen --out ~/.config/kvasir/node-key.json',
    '  ./kvasir-node run --key ~/.config/kvasir/node-key.json --budget 8',
    '',
    '--budget is GPU memory in GiB and has no default. It is the one number',
    'nobody else can choose for you: it decides how much of your card the node',
    'takes, and a machine with other work on it should keep some back.',
    '',
    'Back up the key file. Rewards are paid to its address and it is the only',
    'copy.',
    '',
  ].filter((line) => line !== '').join('\n') + '\n'
}

/**
 * latest.json: what install.sh and `worker --install` read to find out what
 * the current build is and what its bytes should hash to. Assembled from
 * whatever releases are on disk rather than written by hand, so the index and
 * the files cannot disagree.
 */
function index() {
  const entries = fs.existsSync(OUT) ? fs.readdirSync(OUT) : []
  const out = { version: null, generated: new Date().toISOString() }
  for (const name of entries) {
    const node = /^kvasir-node-linux-(x64|arm64)-(.+)\.tar\.gz$/.exec(name)
    if (node) {
      out[`linux-${node[1]}`] = name
      out[`linux-${node[1]}-sha256`] = sha256(path.join(OUT, name))
      out.version = node[2]
      continue
    }
    const worker = /^kvasir-expert-worker-linux-(x64|arm64)-(.+)\.tar\.gz$/.exec(name)
    if (worker) {
      out[`worker-linux-${worker[1]}`] = name
      out[`worker-linux-${worker[1]}-sha256`] = sha256(path.join(OUT, name))
    }
  }
  const file = path.join(OUT, 'latest.json')
  fs.writeFileSync(file, `${JSON.stringify(out, null, 2)}\n`)
  console.log(`${file}\n${JSON.stringify(out, null, 2)}`)
  return out
}

function main() {
  const args = process.argv.slice(2)
  const flag = (name) => {
    const i = args.indexOf(`--${name}`)
    return i >= 0 ? args[i + 1] : null
  }
  fs.mkdirSync(OUT, { recursive: true })
  if (args.includes('--index')) return void index()

  const arch = flag('arch')
  if (!['x64', 'arm64'].includes(arch)) throw new Error('--arch must be x64 or arm64')
  let p4 = flag('p4')
  if (!p4) {
    const bundled = path.join(REPO, 'wallet', 'desktop', 'resources', 'p4', 'linux', 'p4-agent')
    if (fs.existsSync(bundled)) p4 = bundled
  }
  if (!p4) console.warn('no p4-agent given or found — packaging without the layer-serving role')
  build({ arch, p4 })
  index()
}

try { main() } catch (e) { console.error(`package-node-linux: ${e.message}`); process.exit(1) }
