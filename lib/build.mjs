/**
 * `gripforge project build` as a function: project.gripforge → strict check by the common core → the game.json of the
 * built game (scenes lowered for game.engine, dev.builder left out, the game's module in the lock) → the Rust of each
 * scene by the generator of core/scene-format (`init_scene.rs`) → the crate (`index.rs`, `lib.rs`, Cargo) → cargo,
 * native and wasm. Everything lands in `<out>` (default `.build/` beside the project):
 *
 *   .build/game.json        what the players load (no scene file read at run time)
 *   .build/game/            the crate of the game's module (lib/crate.mjs)
 *   .build/native/          its native library + kit-modules-native.json entry
 *   .build/web/             its wasm (relocatable, common memory)
 *   .build/build.json       the report (gripforge.build-report/1): issues, steps, times, artifacts
 *
 * Never throws for a problem of the project: every problem is an issue of the result (file, scene, element,
 * property, what is expected). Used by bin/gripforge-build.mjs, the GripForge CLI and MCP.
 */
import { mkdirSync, writeFileSync } from 'node:fs';
import { basename, dirname, join, resolve } from 'node:path';
import { call } from './core.mjs';
import { BuildError, findKitsDir, kitCatalog, projectFile, readJson, sceneFiles } from './project.mjs';
import { sdkOf, writeCrate } from './crate.mjs';
import { buildNative, buildWasm } from './cargo.mjs';
import { report } from './report.mjs';
import { checkKits, checkSdk } from './compat.mjs';

/** The SDK checkout above `from` (a module-kits clone: core/kit-facade and core/kit-guest), or null. */
export function findSdk(from) {
  const kits = findKitsDir(from);
  return kits ? dirname(kits) : null;
}

/**
 * Builds a project. Options: `project` (file or folder), `out`, `kits` (folder of the kits), `sdk` (module-kits
 * checkout), `targets` (`['native', 'wasm']`), `compile` (false: stop after the crate), `lenient` (the tolerant
 * validator's warnings stay warnings), `jobs`, `quiet`, `targetDir` (cargo's, kept between builds).
 */
export async function build(options = {}) {
  const steps = [], artifacts = {};
  const step = (name, t0, extra = {}) => steps.push({ name, ms: Date.now() - t0, ...extra });
  const fail = (issues, extra = {}) => ({ ok: false, issues, steps, artifacts, ...extra });
  let file;
  try {
    file = projectFile(options.project);
  } catch (e) {
    if (e instanceof BuildError) return fail([e.issue]);
    throw e;
  }
  const dir = dirname(file), projectName = basename(file);
  const out = resolve(options.out ?? join(dir, '.build'));
  let t0 = Date.now(), project, files, kits;
  try {
    project = readJson(file);
    files = sceneFiles(project, dir);
    kits = kitCatalog(options.kits ?? findKitsDir(dir));
  } catch (e) {
    if (e instanceof BuildError) return fail([e.issue], { projectName });
    throw e;
  }
  step('read', t0, { kits: kits.length, files: Object.keys(files).length });
  const sdkPath = options.sdk ? resolve(options.sdk) : findSdk(dir);
  const incompatible = [...checkKits(kits), ...(sdkPath && options.compile !== false ? checkSdk(sdkPath) : [])];
  if (incompatible.length) return fail(incompatible, { projectName, project: file });

  t0 = Date.now();
  const answer = call({ op: 'code', project, files, kits, lenient: !!options.lenient });
  step('check+generate', t0, { errors: answer.errors ?? 0 });
  const issues = answer.issues ?? [];
  if (!answer.ok) return fail(issues, { projectName, project: file });

  mkdirSync(out, { recursive: true });
  writeFileSync(join(out, 'game.json'), `${JSON.stringify(answer.game, null, 1)}\n`);
  artifacts.game = join(out, 'game.json');
  const sdk = sdkOf(sdkPath, options.sdkRev);
  const crate = writeCrate(join(out, 'game'), {
    projectId: String(project.id ?? 'game'),
    version: String(project.version ?? '0.1.0'),
    sdk,
    generated: answer.files ?? [],
    entry: answer.entry,
  });
  artifacts.crate = crate.dir;
  const result = { ok: true, issues, steps, artifacts, projectName, project: file, out };
  if (options.compile !== false) {
    const targets = options.targets ?? ['native', 'wasm'];
    const opts = { jobs: options.jobs ?? 4, quiet: options.quiet, targetDir: options.targetDir, sdkPath };
    for (const target of targets) {
      t0 = Date.now();
      try {
        const r = target === 'wasm' ? await buildWasm(crate.dir, out, opts) : buildNative(crate.dir, out, opts);
        artifacts[target] = r;
        step(`compile ${target}`, t0, { bytes: r.bytes });
      } catch (e) {
        if (!e.issue) throw e;
        result.ok = false;
        issues.push(e.issue);
        step(`compile ${target}`, t0, { failed: true });
      }
    }
  }
  writeFileSync(join(out, 'build.json'), `${JSON.stringify(report(result), null, 1)}\n`);
  return result;
}
