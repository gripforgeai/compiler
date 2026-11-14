/**
 * Compiles the game's crate (lib/crate.mjs) with cargo, at low priority (`nice -n 15`, `-j 4` by default): a native
 * library for the native players and engines, and a wasm for the web player, built RELOCATABLE like every kit module
 * of the common memory (the recipe of the SDK's tools/kit-modules/build.mjs, read from it, never copied). The results
 * go to `<out>/native/` (library + kit-modules-native.json entry) and `<out>/web/` (wasm + its card).
 *
 * Used by lib/build.mjs.
 */
import { spawnSync } from 'node:child_process';
import { copyFileSync, existsSync, mkdirSync, readFileSync, statSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { pathToFileURL } from 'node:url';
import { GAME_MODULE, LIBRARY } from './crate.mjs';

/** The file of a native library on this system. */
export const libraryFile = (name) => (process.platform === 'win32' ? `${name}.dll` : process.platform === 'darwin' ? `lib${name}.dylib` : `lib${name}.so`);

/** Runs cargo in the crate; throws an issue-shaped error with the compiler's own messages when it fails. */
function cargo(dir, args, env, { jobs, quiet }) {
  const cmd = process.platform === 'win32' ? 'cargo' : 'nice';
  const all = process.platform === 'win32' ? [...args, '-j', String(jobs)] : ['-n', '15', process.env.CARGO ?? 'cargo', ...args, '-j', String(jobs)];
  const r = spawnSync(cmd, all, { cwd: dir, env: { ...process.env, ...env }, encoding: 'utf8', stdio: quiet ? 'pipe' : ['ignore', 'inherit', 'pipe'], maxBuffer: 64 << 20 });
  if (r.error) throw Object.assign(Error(r.error.message), { issue: { level: 'error', code: 'cargo_missing', message: `cargo could not run: ${r.error.message}`, fix: 'install Rust (rustup) and its target wasm32-unknown-unknown with the component rust-src' } });
  // 127: the shell (nice) found no cargo.
  if (r.status === 127) throw Object.assign(Error('no cargo'), { issue: { level: 'error', code: 'cargo_missing', message: `cargo could not run: ${(r.stderr ?? '').trim().split('\n').pop()}`, fix: 'install Rust (rustup) and its target wasm32-unknown-unknown with the component rust-src' } });
  if (r.status !== 0) {
    if (/failed to (load manifest|get) .*gripforge-kit-(facade|guest)/.test(r.stderr ?? '')) {
      throw Object.assign(Error('no sdk'), { issue: { level: 'error', code: 'sdk_missing', file: join(dir, 'Cargo.toml'), message: `the crates of the SDK (gripforge-kit-guest, gripforge-kit-facade) could not be read: ${(r.stderr ?? '').split('\n').find((l) => l.startsWith('error')) ?? ''}`, fix: 'give --sdk <a module-kits checkout> (with core/kit-facade), or --sdk-rev <a commit of module-kits that has it>' } });
    }
    const errs = (r.stderr ?? '').split('\n').filter((l) => /^(error|warning: unused)/.test(l) || /^\s+-->/.test(l)).slice(0, 30).join('\n');
    throw Object.assign(Error('cargo failed'), { issue: { level: 'error', code: 'rust_build_failed', file: join(dir, 'Cargo.toml'), message: `cargo ${args[0]} failed (exit ${r.status}):\n${errs || (r.stderr ?? '').slice(-3000)}`, fix: 'the generated code did not compile: report it with this output (a generator or façade bug, never to fix by hand in .build/)' } });
  }
  return r;
}

/** The native library, copied to `<out>/native/` with its kit-modules-native.json entry. */
export function buildNative(crateDir, out, { profile = 'release', jobs = 4, quiet = false, targetDir } = {}) {
  const t0 = Date.now();
  const target = targetDir ?? join(crateDir, 'target');
  cargo(crateDir, ['build', '--lib', '--profile', profile], { CARGO_TARGET_DIR: target }, { jobs, quiet });
  const lib = libraryFile(LIBRARY), built = join(target, profile === 'dev' ? 'debug' : profile, lib);
  const dir = join(out, 'native');
  mkdirSync(dir, { recursive: true });
  copyFileSync(built, join(dir, lib));
  copyFileSync(join(crateDir, 'kit.api.json'), join(dir, `${GAME_MODULE}.kit.api.json`));
  const api = JSON.parse(readFileSync(join(crateDir, 'kit.api.json'), 'utf8'));
  const module = JSON.parse(readFileSync(join(crateDir, 'module.json'), 'utf8'));
  const manifest = { contract: 'kithost.native@2', kits: { [GAME_MODULE]: { library: lib, api: `${GAME_MODULE}.kit.api.json`, version: api.version, bytes: statSync(join(dir, lib)).size, implementations: module.implementations } } };
  writeFileSync(join(dir, 'kit-modules-native.json'), `${JSON.stringify(manifest, null, 1)}\n`);
  return { library: join(dir, lib), manifest: join(dir, 'kit-modules-native.json'), bytes: statSync(join(dir, lib)).size, ms: Date.now() - t0 };
}

/** The wasm of the web, relocatable for the common memory, with the SDK's recipe (needs a local SDK checkout). */
export async function buildWasm(crateDir, out, { sdkPath, jobs = 4, quiet = false, targetDir } = {}) {
  const t0 = Date.now();
  const recipe = sdkPath && join(sdkPath, 'tools/kit-modules/build.mjs');
  if (!recipe || !existsSync(recipe)) {
    throw Object.assign(Error('no recipe'), { issue: { level: 'error', code: 'wasm_recipe_missing', message: 'the wasm of the web is built with the recipe of the SDK (tools/kit-modules/build.mjs), and no SDK checkout was given', fix: 'give --sdk <module-kits checkout>, or build --target native only' } });
  }
  const { RELOCATABLE } = await import(pathToFileURL(recipe).href);
  const socle = JSON.parse(readFileSync(join(sdkPath, 'core/kit-guest/socle.json'), 'utf8')).profile;
  const profile = Object.entries({ inherits: 'release', ...socle }).flatMap(([k, v]) => ['--config', `profile.kit-module.${k}=${JSON.stringify(v)}`]);
  const target = join(targetDir ?? join(crateDir, 'target'), 'pic');
  cargo(crateDir, ['build', '--lib', '--profile', 'kit-module', ...profile, '--target', 'wasm32-unknown-unknown', ...RELOCATABLE.cargoArgs],
    { ...RELOCATABLE.env, RUSTFLAGS: RELOCATABLE.rustflags, CARGO_TARGET_DIR: target }, { jobs, quiet });
  const built = join(target, 'wasm32-unknown-unknown/kit-module', `${LIBRARY}.wasm`);
  const dir = join(out, 'web');
  mkdirSync(dir, { recursive: true });
  const file = join(dir, `${GAME_MODULE}.wasm`);
  copyFileSync(built, file);
  copyFileSync(join(crateDir, 'kit.api.json'), join(dir, `${GAME_MODULE}.kit.api.json`));
  return { wasm: file, bytes: statSync(file).size, ms: Date.now() - t0 };
}
