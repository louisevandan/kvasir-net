'use strict'
/**
 * Give the Linux tarball its execute bits back.
 *
 * electron-builder takes file modes from the filesystem it packs from. On
 * Windows there are no execute bits, so a Linux tar.gz built there stores
 * every file as 0644 — the app binary, chrome-sandbox, the .so files and the
 * p4 agent included. It unpacks on Linux into an app that cannot start:
 * "permission denied" on the very first click.
 *
 * This rewrites the archive's headers in place: any regular file whose
 * content starts with an ELF magic or a "#!" line gets 0755; everything else
 * keeps its mode. Contents are untouched. It runs as electron-builder's
 * artifactBuildCompleted hook and also stands alone:
 *
 *   node scripts/fixLinuxModes.cjs release/Kvasir-Wallet-0.1.0-linux-x64.tar.gz
 *
 * It is a no-op for archives already built on Linux or macOS (the modes are
 * already right) and for any artifact that is not a .tar.gz.
 */
const fs = require('node:fs')
const zlib = require('node:zlib')

const BLOCK = 512
const ELF = Buffer.from([0x7f, 0x45, 0x4c, 0x46])

const octal = (buf, off, len) => parseInt(buf.toString('ascii', off, off + len).replace(/\0.*$/, '').trim() || '0', 8)

function writeOctal(buf, off, len, value) {
  // len-1 digits, then NUL — the classic ustar layout.
  const s = value.toString(8).padStart(len - 1, '0')
  buf.write(s, off, len - 1, 'ascii')
  buf[off + len - 1] = 0
}

function checksum(header) {
  let sum = 0
  for (let i = 0; i < BLOCK; i++) sum += (i >= 148 && i < 156) ? 0x20 : header[i]
  return sum
}

/** Returns the number of files made executable. Mutates `tar` in place. */
function fixModes(tar) {
  let fixed = 0
  let off = 0
  let paxSize = null   // a pax 'x' header can override the next entry's size
  while (off + BLOCK <= tar.length) {
    const header = tar.subarray(off, off + BLOCK)
    if (header.every((b) => b === 0)) break   // end of archive
    let size = octal(header, 124, 12)
    const type = String.fromCharCode(header[156] || 0x30)
    if (paxSize != null && (type === '0' || type === '\0')) { size = paxSize }
    const dataOff = off + BLOCK
    if (type === 'x') {
      const m = /(?:^|\n)\d+ size=(\d+)\n/.exec(tar.toString('utf8', dataOff, dataOff + size))
      paxSize = m ? Number(m[1]) : null
    } else if (type !== 'g' && type !== 'L' && type !== 'K') {
      if ((type === '0' || header[156] === 0) && size >= 4) {
        const head = tar.subarray(dataOff, dataOff + 4)
        const executable = head.equals(ELF) || (head[0] === 0x23 && head[1] === 0x21)
        const mode = octal(header, 100, 8)
        if (executable && (mode & 0o111) === 0) {
          writeOctal(header, 100, 8, (mode & 0o7000) | 0o755)
          writeOctal(header, 148, 7, checksum(header))
          header[155] = 0x20
          fixed++
        }
      }
      paxSize = null
    }
    off = dataOff + Math.ceil(size / BLOCK) * BLOCK
  }
  return fixed
}

function fixTarGz(file) {
  const tar = zlib.gunzipSync(fs.readFileSync(file))
  const fixed = fixModes(tar)
  if (fixed) fs.writeFileSync(file, zlib.gzipSync(tar, { level: 9 }))
  return fixed
}

// electron-builder hook: called once per artifact.
module.exports = async function artifactBuildCompleted(context) {
  const file = context && context.file
  if (!file || !file.endsWith('.tar.gz')) return
  const fixed = fixTarGz(file)
  console.log(`  [fixLinuxModes] ${fixed} file(s) marked executable in ${require('node:path').basename(file)}`)
}
module.exports.fixModes = fixModes
module.exports.fixTarGz = fixTarGz

if (require.main === module) {
  for (const f of process.argv.slice(2)) console.log(`${f}: ${fixTarGz(f)} file(s) marked executable`)
}
