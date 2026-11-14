#!/usr/bin/env node
/**
 * Puts the kits SDK (module-kits) in `.sdk/module-kits`: the common core the compiler is built from
 * (core/scene-format), the kits' catalog the tests read (kits/<kit>/kit.api.json) and the crates a built game links
 * (core/kit-guest, core/kit-facade). Nothing of it is copied into this repository.
 *
 *   node scripts/sdk.mjs                 fetch module-kits at the revision of sdk.json (git, shallow), if not there yet
 *   node scripts/sdk.mjs --link <dir>    use an existing module-kits checkout instead (a symbolic link, nothing fetched)
 *   node scripts/sdk.mjs --check         only say whether .sdk/module-kits is at the revision of sdk.json
 *
 * git reads the repository with the credentials it already has (a clone of a private repository needs a read
 * access). In CI the workflow checks the SDK out itself and this script only checks it.
 */
import { execFileSync } from 'node:child_process';
import { existsSync, lstatSync, mkdirSync, readFileSync, rmSync, symlinkSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const pin = JSON.parse(readFileSync(join(root, 'sdk.json'), 'utf8'));
const dir = join(root, '.sdk', 'module-kits');
const git = (args, cwd = dir) => execFileSync('git', args, { cwd, encoding: 'utf8', stdio: ['ignore', 'pipe', 'inherit'] }).trim();
const head = () => {
  try { return git(['rev-parse', 'HEAD']); } catch { return null; }
};

const args = process.argv.slice(2);
const at = args.indexOf('--link');
if (at >= 0) {
  const target = resolve(args[at + 1] ?? '');
  if (!existsSync(join(target, 'core', 'scene-format'))) {
    console.error(`sdk: ${target} is not a module-kits checkout (no core/scene-format)`);
    process.exit(2);
  }
  mkdirSync(dirname(dir), { recursive: true });
  if (existsSync(dir) || isLink(dir)) rmSync(dir, { recursive: true, force: true });
  symlinkSync(target, dir, 'dir');
  const h = head();
  console.log(`sdk: .sdk/module-kits → ${target} (${h ?? 'not a git checkout'})${h && h !== pin.rev ? `; sdk.json pins ${pin.rev}: the golden tests may differ` : ''}`);
  process.exit(0);
}
if (args.includes('--check')) {
  const h = existsSync(dir) ? head() : null;
  if (h === pin.rev) {
    console.log(`sdk: .sdk/module-kits at ${pin.rev}`);
    process.exit(0);
  }
  console.error(h ? `sdk: .sdk/module-kits is at ${h}, sdk.json pins ${pin.rev}` : 'sdk: no .sdk/module-kits (npm run sdk)');
  process.exit(1);
}
if (existsSync(dir) && head() === pin.rev) {
  console.log(`sdk: .sdk/module-kits already at ${pin.rev}`);
  process.exit(0);
}
if (existsSync(dir) && isLink(dir)) {
  console.log(`sdk: .sdk/module-kits is a link to a checkout at ${head()} (sdk.json pins ${pin.rev}); kept`);
  process.exit(0);
}
mkdirSync(dir, { recursive: true });
if (!existsSync(join(dir, '.git'))) git(['init', '-q']);
try { git(['remote', 'add', 'origin', pin.repository]); } catch { git(['remote', 'set-url', 'origin', pin.repository]); }
git(['fetch', '-q', '--depth', '1', 'origin', pin.rev]);
git(['checkout', '-q', '--force', 'FETCH_HEAD']);
console.log(`sdk: .sdk/module-kits at ${head()}`);

function isLink(p) {
  try { return lstatSync(p).isSymbolicLink(); } catch { return false; }
}
