/**
 * electron-builder afterPack hook — fix an asar dependency-collection bug.
 *
 * WHY THIS EXISTS
 * ---------------
 * The @solana v2 stack pulls in two different major lines of the low-level
 * codec packages: the release-candidate line (`2.0.0-rc.1`, required by
 * @solana/codecs, @solana/options, @solana/spl-token) hoisted to the TOP of
 * node_modules, and the newer line (`2.3.0`, required by @solana/web3.js)
 * nested under @solana/web3.js/node_modules. electron-builder 25's node_modules
 * collector mis-resolves this shape: it places the rc.1 `@solana/codecs-core`
 * (and codecs-numbers / codecs-strings / codecs-data-structures) ONLY nested
 * under @solana/codecs, and drops the TOP-LEVEL copy. At runtime
 * `@solana/options` (which has no nested node_modules) can no longer resolve
 * `@solana/codecs-core` and the app crashes on launch:
 *
 *     Uncaught Exception: Error: Cannot find module '@solana/codecs-core'
 *     Require stack: .../@solana/options/dist/index.node.cjs
 *
 * Declaring the packages as explicit dependencies in package.json is not enough
 * — the collector still prunes the top-level copies because it considers them
 * duplicates of the nested ones. The `files: ["node_modules/**"]` glob does not
 * help either: electron-builder ignores node_modules globs and uses its own
 * collector output.
 *
 * WHAT THIS DOES
 * --------------
 * After electron-builder packs the app (but before it builds the DMG/NSIS/
 * tar.gz installer), we open the produced app.asar and, for each package in
 * REQUIRE_TOP_LEVEL that is missing at the top level, inject it from the
 * project's own node_modules. The repack preserves the same native-module
 * unpack set electron-builder uses, so *.node stay in app.asar.unpacked and
 * load via dlopen. The operation is idempotent (already-present packages are
 * skipped), so it is safe across the multiple invocations electron-builder
 * makes for a macOS universal build.
 */
const fs = require('fs');
const path = require('path');
const asar = require('@electron/asar');

// Packages that must exist at the TOP level of the asar node_modules but that
// electron-builder's collector drops (rc.1 codec family) or has historically
// dropped. Injected only when missing, from <projectDir>/node_modules.
const REQUIRE_TOP_LEVEL = [
  '@solana/codecs-core',
  '@solana/codecs-numbers',
  '@solana/codecs-strings',
  '@solana/codecs-data-structures',
  'call-bind-apply-helpers',
];

// Native modules electron-builder keeps unpacked. Must match its default so the
// repacked asar keeps *.node outside the archive (dlopen cannot read an asar).
const UNPACK_DIR = '**/node_modules/{bigint-buffer,bufferutil,utf-8-validate}';
const UNPACK = '**/*.node';

module.exports = async function afterPack(context) {
  const { appOutDir, packager, electronPlatformName } = context;
  const projectDir = packager.projectDir;
  const productName = packager.appInfo.productFilename;

  const resourcesDir =
    electronPlatformName === 'darwin'
      ? path.join(appOutDir, `${productName}.app`, 'Contents', 'Resources')
      : path.join(appOutDir, 'resources');
  const asarPath = path.join(resourcesDir, 'app.asar');
  if (!fs.existsSync(asarPath)) return; // asar disabled — nothing to patch

  // @electron/asar caches parsed archives per path within a process; drop any
  // stale entry so we read the freshly-written asar (and re-read after repack).
  const uncache = () => {
    if (typeof asar.uncache === 'function') asar.uncache(asarPath);
    else if (typeof asar.uncacheAll === 'function') asar.uncacheAll();
  };
  uncache();

  const top = new Set(
    asar
      .listPackage(asarPath)
      .filter((p) => /^\/node_modules\/(@[^/]+\/)?[^/]+$/.test(p))
      .map((p) => p.replace(/^\/node_modules\//, '')),
  );

  const missing = REQUIRE_TOP_LEVEL.filter((pkg) => {
    if (top.has(pkg)) return false;
    const src = path.join(projectDir, 'node_modules', pkg);
    if (!fs.existsSync(path.join(src, 'package.json'))) {
      console.warn(`  [afterPack] ${pkg} missing from asar AND from node_modules — skipping`);
      return false;
    }
    return true;
  });

  if (missing.length === 0) return; // idempotent: already complete

  console.log(`  [afterPack] injecting top-level packages dropped by the collector: ${missing.join(', ')}`);

  const work = path.join(appOutDir, '.asar-patch-tmp');
  fs.rmSync(work, { recursive: true, force: true });
  asar.extractAll(asarPath, work);
  for (const pkg of missing) {
    fs.cpSync(path.join(projectDir, 'node_modules', pkg), path.join(work, 'node_modules', pkg), {
      recursive: true,
    });
  }
  fs.rmSync(asarPath, { force: true });
  fs.rmSync(`${asarPath}.unpacked`, { recursive: true, force: true });
  await asar.createPackageWithOptions(work, asarPath, { unpackDir: UNPACK_DIR, unpack: UNPACK });
  fs.rmSync(work, { recursive: true, force: true });

  uncache(); // repacked asar replaced the cached one at the same path
  const after = asar
    .listPackage(asarPath)
    .filter((p) => /^\/node_modules\/(@[^/]+\/)?[^/]+$/.test(p))
    .map((p) => p.replace(/^\/node_modules\//, ''));
  const still = REQUIRE_TOP_LEVEL.filter((p) => !after.includes(p));
  if (still.length) throw new Error(`[afterPack] injection failed, still missing: ${still.join(', ')}`);
  console.log('  [afterPack] app.asar dependency injection complete');
};
