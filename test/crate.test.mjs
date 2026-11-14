/**
 * Step 5, the crate of the game's module (lib/crate.mjs): Cargo.toml (SDK by path, by git at a commit or a branch),
 * lib.rs, index.rs, the module's card, its implementations, the writing of the folder (stale scenes removed, nothing
 * written outside src/), the SDK of a checkout or of git.
 */
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { existsSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { GAME_MODULE, LIBRARY, SDK_GIT, cargoToml, indexRs, kitApi, libRs, moduleJson, sdkOf, writeCrate } from '../lib/crate.mjs';
import { ROOT, tmp } from './helpers.mjs';
import { sdkPin } from '../lib/compat.mjs';

test('Cargo.toml by path: its own workspace, the library name the hosts load, the façades with the guest side', () => {
  const t = cargoToml('my_game', '1.2.3', { path: '/sdk' });
  assert.match(t, /^name = "gripforge-game-my-game"$/m);
  assert.match(t, /^version = "1\.2\.3"$/m);
  assert.match(t, new RegExp(`^name = "${LIBRARY}"$`, 'm'));
  assert.match(t, /crate-type = \["cdylib", "rlib"\]/);
  assert.match(t, /^gripforge-kit-guest = \{ path = "\/sdk\/core\/kit-guest" \}$/m);
  assert.match(t, /^gripforge-kit-facade = \{ path = "\/sdk\/core\/kit-facade", features = \["guest"\] \}$/m);
  assert.match(t, /^\[workspace\]$/m);
});

test('Cargo.toml by git: a commit is a rev, a branch is a branch', () => {
  const rev = cargoToml('g', '0.1.0', { git: SDK_GIT, rev: 'abcdef1' });
  assert.match(rev, /gripforge-kit-guest = \{ git = "https:\/\/github\.com\/gripforgeai\/module-kits", rev = "abcdef1" \}/);
  const branch = cargoToml('g', '0.1.0', { rev: 'feat_refonte-kits' });
  assert.match(branch, /branch = "feat_refonte-kits"/);
});

test('lib.rs exports a kit module; index.rs enters through the generator\'s entry and gives back at the exit', () => {
  assert.match(libRs(), /export_kit!\(index::module, env!\("CARGO_PKG_VERSION"\)\);/);
  assert.match(libRs(), /pub mod init_scene;/);
  const index = indexRs();
  assert.match(index, /crate::init_scene::enter\(kx, scene\["id"\]/);
  assert.match(index, new RegExp(`"${GAME_MODULE.replace('.', '\\.')}"`));
  assert.match(index, /posed\.leave\(kx\)/);
  assert.match(indexRs('scenes::pose'), /crate::scenes::pose\(kx/);
});

test('the card and the implementations of game.main', () => {
  const api = kitApi('2.0.0');
  assert.equal(api.id, 'game.main');
  assert.equal(api.version, '2.0.0');
  assert.equal(api.schema, 'gripforge.kit-api/1');
  const m = moduleJson();
  assert.equal(m.implementations['bevy-player'].library, LIBRARY);
  assert.equal(m.implementations['bevy-web'].form, 'wasm');
});

test('writeCrate writes every file, removes the scenes of a previous build, keeps nothing outside src/', () => {
  const dir = tmp();
  mkdirSync(join(dir, 'src', 'init_scene'), { recursive: true });
  writeFileSync(join(dir, 'src', 'init_scene', 'old.rs'), '// stale');
  const r = writeCrate(dir, { projectId: 'g', version: '0.1.0', sdk: { path: '/sdk', toolchain: '[toolchain]\nchannel = "1.99.0"\n' }, generated: [{ path: 'init_scene.rs', text: 'pub mod a;' }, { path: 'init_scene/a.rs', text: '// a' }], entry: 'init_scene::enter' });
  assert.deepEqual(r.files, ['Cargo.toml', 'kit.api.json', 'module.json', 'src/lib.rs', 'src/index.rs', 'src/init_scene.rs', 'src/init_scene/a.rs']);
  assert.equal(existsSync(join(dir, 'src', 'init_scene', 'old.rs')), false);
  assert.match(readFileSync(join(dir, 'rust-toolchain.toml'), 'utf8'), /1\.99\.0/);
  assert.throws(() => writeCrate(dir, { projectId: 'g', version: '0', sdk: { path: '/s' }, generated: [{ path: '../evil.rs', text: '' }] }), /outside src/);
});

test('the SDK: a checkout (with its toolchain), git at the pinned revision by default, git from the environment', () => {
  const local = sdkOf(ROOT);
  assert.equal(local.path, ROOT);
  assert.match(local.toolchain, /channel/);
  assert.equal(sdkOf(tmp()).toolchain, null);
  const git = sdkOf(null);
  assert.equal(git.git, SDK_GIT);
  assert.equal(git.rev, sdkPin().rev, 'by default, the SDK revision this compiler is made for (sdk.json)');
  assert.equal(sdkOf(null, 'abc1234').rev, 'abc1234');
  process.env.GRIPFORGE_SDK_REV = 'deadbeef';
  try {
    assert.equal(sdkOf(null).rev, 'deadbeef');
  } finally {
    delete process.env.GRIPFORGE_SDK_REV;
  }
});
