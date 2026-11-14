/**
 * Step 6, compilation and the whole command (lib/cargo.mjs, lib/build.mjs), with a fake cargo (test/helpers.mjs) so
 * that what surrounds cargo is tested in a second: the arguments (nice, -j, profiles, the wasm recipe of the SDK),
 * where the results land, the manifest of the native module, each failure turned into an issue. The real compilation
 * is proved by kits/dev_builder/tests/compiled.rs (cargo test), and by GF_COMPILER_CARGO=1 below.
 */
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { existsSync, readFileSync } from 'node:fs';
import { join } from 'node:path';
import { build, findSdk } from '../lib/build.mjs';
import { buildNative, buildWasm, libraryFile } from '../lib/cargo.mjs';
import { writeCrate } from '../lib/crate.mjs';
import { KITS, ROOM_DIR, ROOT, fakeCargo, projectDir, room, scene, tmp, withCargo } from './helpers.mjs';

const crate = () => {
  const dir = tmp();
  writeCrate(dir, { projectId: 'g', version: '0.3.0', sdk: { path: ROOT }, generated: [] });
  return dir;
};

test('native: cargo under nice with -j, the library and its kit-modules-native.json in native/', async () => {
  const cargo = fakeCargo();
  const dir = crate(), out = tmp();
  const r = await withCargo(cargo.bin, () => buildNative(dir, out, { jobs: 3, quiet: true }));
  const [call] = cargo.calls();
  assert.deepEqual(call.args, ['build', '--lib', '--profile', 'release', '-j', '3']);
  assert.equal(call.cwd, dir);
  assert.equal(call.env.CARGO_TARGET_DIR, join(dir, 'target'));
  assert.ok(existsSync(r.library));
  const m = JSON.parse(readFileSync(join(out, 'native', 'kit-modules-native.json'), 'utf8'));
  assert.equal(m.contract, 'kithost.native@2');
  assert.equal(m.kits['game.main'].library, libraryFile('gfkit_game_main'));
  assert.equal(m.kits['game.main'].version, '0.3.0');
  assert.ok(existsSync(join(out, 'native', 'game.main.kit.api.json')));
});

test('native: another profile and target folder', async () => {
  const cargo = fakeCargo();
  const target = tmp();
  await withCargo(cargo.bin, () => buildNative(crate(), tmp(), { profile: 'dev', targetDir: target, quiet: true }));
  assert.equal(cargo.calls()[0].env.CARGO_TARGET_DIR, target);
});

test('wasm: the relocatable recipe and the kit profile of the SDK, the module in web/', async () => {
  const cargo = fakeCargo();
  const out = tmp();
  const r = await withCargo(cargo.bin, () => buildWasm(crate(), out, { sdkPath: ROOT, quiet: true }));
  const [call] = cargo.calls();
  assert.ok(call.args.includes('wasm32-unknown-unknown'));
  assert.ok(call.args.some((a) => a.startsWith('-Zbuild-std')));
  assert.ok(call.args.includes('profile.kit-module.panic="abort"'));
  assert.match(call.env.RUSTFLAGS, /relocation-model=pic/);
  assert.equal(call.env.RUSTC_BOOTSTRAP, '1');
  assert.match(call.env.CARGO_TARGET_DIR, /pic$/);
  assert.equal(r.wasm, join(out, 'web', 'game.main.wasm'));
  assert.ok(existsSync(join(out, 'web', 'game.main.kit.api.json')));
});

test('wasm without the recipe of an SDK: wasm_recipe_missing', async () => {
  await assert.rejects(buildWasm(crate(), tmp(), { sdkPath: tmp() }), (e) => e.issue?.code === 'wasm_recipe_missing');
  await assert.rejects(buildWasm(crate(), tmp(), {}), (e) => e.issue?.code === 'wasm_recipe_missing');
});

test('a failed compilation: rust_build_failed with the compiler\'s lines', async () => {
  const cargo = fakeCargo('fail');
  await assert.rejects(withCargo(cargo.bin, () => buildNative(crate(), tmp(), { quiet: true })), (e) => {
    assert.equal(e.issue.code, 'rust_build_failed');
    assert.match(e.issue.message, /error\[E0425\]/);
    assert.match(e.issue.message, /--> src\/init_scene\/level\.rs:3:5/);
    assert.match(e.issue.fix, /never to fix by hand/);
    return true;
  });
});

test('the SDK\'s crates missing: sdk_missing; cargo missing: cargo_missing', async () => {
  await assert.rejects(withCargo(fakeCargo('nosdk').bin, () => buildNative(crate(), tmp(), { quiet: true })), (e) => e.issue?.code === 'sdk_missing');
  await assert.rejects(withCargo(join(tmp(), 'no-cargo'), () => buildNative(crate(), tmp(), { quiet: true })), (e) => e.issue?.code === 'cargo_missing');
});

test('build: the room, native and wasm, every artifact and the report', async () => {
  const cargo = fakeCargo();
  const out = tmp();
  const r = await withCargo(cargo.bin, () => build({ project: ROOM_DIR, out, quiet: true }));
  assert.equal(r.ok, true, JSON.stringify(r.issues, null, 1));
  assert.deepEqual(r.steps.map((s) => s.name), ['read', 'check+generate', 'compile native', 'compile wasm']);
  for (const f of ['game.json', 'build.json', 'game/src/index.rs', 'game/src/init_scene/level.rs', 'native/kit-modules-native.json', 'web/game.main.wasm']) {
    assert.ok(existsSync(join(out, f)), f);
  }
  const rep = JSON.parse(readFileSync(join(out, 'build.json'), 'utf8'));
  assert.equal(rep.ok, true);
  assert.equal(rep.errors, 0);
  assert.equal(cargo.calls().length, 2);
});

test('build: a compilation that fails makes the build fail, the report says it', async () => {
  const out = tmp();
  const r = await withCargo(fakeCargo('fail').bin, () => build({ project: ROOM_DIR, out, targets: ['native'], quiet: true }));
  assert.equal(r.ok, false);
  assert.equal(r.issues.at(-1).code, 'rust_build_failed');
  assert.equal(r.steps.at(-1).failed, true);
  assert.equal(JSON.parse(readFileSync(join(out, 'build.json'), 'utf8')).ok, false);
});

test('build: no compile writes the crate only; the default out is .build beside the project', async () => {
  const d = projectDir(room());
  const r = await build({ project: d, compile: false, kits: KITS });
  assert.equal(r.ok, true, JSON.stringify(r.issues));
  assert.equal(r.out, join(d, '.build'));
  assert.ok(existsSync(join(d, '.build', 'game', 'Cargo.toml')));
  assert.equal(existsSync(join(d, '.build', 'native')), false);
});

test('build: an invalid project, a missing one, one that is not JSON, no catalog: issues, nothing written', async () => {
  const p = room();
  scene(p, 'level').kits['core.light'].brightness = 2;
  const d = projectDir(p);
  const bad = await build({ project: d, kits: KITS });
  assert.equal(bad.ok, false);
  assert.equal(existsSync(join(d, '.build')), false);
  assert.equal((await build({ project: join(tmp(), 'none') })).issues[0].code, 'project_missing');
  assert.equal((await build({ project: projectDir('{ nope') })).issues[0].code, 'file_not_json');
  assert.equal((await build({ project: projectDir(room()), kits: join(tmp(), 'none') })).issues[0].code, 'kits_missing');
});

test('the SDK above a project, none elsewhere', () => {
  assert.equal(findSdk(ROOM_DIR), ROOT);
  assert.equal(findSdk(tmp()), null);
});

test('a real native compilation of the room (GF_COMPILER_CARGO=1, a few seconds with a warm cache)', { skip: process.env.GF_COMPILER_CARGO !== '1' && 'set GF_COMPILER_CARGO=1 to run cargo for real' }, async () => {
  const out = tmp();
  const r = await build({ project: ROOM_DIR, out, targets: ['native'], quiet: true, targetDir: process.env.GF_COMPILER_TARGET_DIR });
  assert.equal(r.ok, true, JSON.stringify(r.issues, null, 1));
  assert.ok(r.artifacts.native.bytes > 10_000);
});
