#!/usr/bin/env node
/**
 * Prepares the package from this repository and the kits SDK (.sdk/module-kits, `npm run sdk`):
 *   dist/gripforge_compiler.wasm   the Rust core (wasm/, release, wasm32-unknown-unknown, `nice -n 15`, `-j 4`)
 *   dist/kits.json                 the catalog of the kits (every kits/<kit>/kit.api.json of the SDK), used when no
 *                                  kits folder is beside the project
 *   dist/sdk.json                  the SDK the package was made with (repository, revision) and what it needs of
 *                                  the kits (lib/compat.mjs checks it at each build)
 *
 *   node scripts/prepare.mjs [--release] [--if-missing]
 *
 * Run by `npm pack` (prepack, --release) and before the tests (--if-missing: only what is missing). An installed
 * package carries dist/ and never runs it.
 */
import { execFileSync } from 'node:child_process';
import { copyFileSync, existsSync, mkdirSync, readdirSync, readFileSync, writeFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const pkg = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const sdk = resolve(process.env.GRIPFORGE_SDK_DIR ?? join(pkg, '.sdk', 'module-kits'));
const dist = join(pkg, 'dist');
const ifMissing = process.argv.includes('--if-missing');
const pin = JSON.parse(readFileSync(join(pkg, 'sdk.json'), 'utf8'));
if (!existsSync(join(pkg, 'wasm', 'Cargo.toml'))) {
  console.error('prepare: not in the compiler repository, nothing to build (the package carries dist/)');
  process.exit(0);
}
if (!existsSync(join(sdk, 'core', 'scene-format'))) {
  console.error(`prepare: no kits SDK in ${sdk}: run "npm run sdk" first (module-kits at the revision of sdk.json)`);
  process.exit(1);
}
mkdirSync(dist, { recursive: true });
const wasm = join(dist, 'gripforge_compiler.wasm');
if (!(ifMissing && existsSync(wasm))) {
  const target = process.env.CARGO_TARGET_DIR ?? join(pkg, 'wasm', 'target');
  const cargo = process.env.CARGO ?? 'cargo';
  const args = ['build', '--manifest-path', join(pkg, 'wasm', 'Cargo.toml'), '--release', '--target', 'wasm32-unknown-unknown', '-j', process.env.GF_JOBS ?? '4'];
  if (process.platform === 'win32') execFileSync(cargo, args, { cwd: pkg, stdio: ['ignore', 2, 2] });
  else execFileSync('nice', ['-n', '15', cargo, ...args], { cwd: pkg, stdio: ['ignore', 2, 2] });
  copyFileSync(join(target, 'wasm32-unknown-unknown/release/gripforge_compiler_wasm.wasm'), wasm);
}
let rev = null;
try { rev = execFileSync('git', ['rev-parse', 'HEAD'], { cwd: sdk, encoding: 'utf8' }).trim(); } catch { /* not a clone */ }
const catalog = join(dist, 'kits.json');
if (!(ifMissing && existsSync(catalog))) {
  const kits = readdirSync(join(sdk, 'kits')).sort().filter((k) => existsSync(join(sdk, 'kits', k, 'kit.api.json')))
    .map((k) => JSON.parse(readFileSync(join(sdk, 'kits', k, 'kit.api.json'), 'utf8')));
  writeFileSync(catalog, `${JSON.stringify({ schema: 'gripforge.kit-catalog/1', commit: rev, kits })}\n`);
}
writeFileSync(join(dist, 'sdk.json'), `${JSON.stringify({ repository: pin.repository, rev: rev ?? pin.rev, compat: pin.compat }, null, 1)}\n`);
if (rev && rev !== pin.rev) console.warn(`prepare: the SDK is at ${rev}, sdk.json pins ${pin.rev}`);
console.error(`prepare: ${wasm}, ${catalog}`);
