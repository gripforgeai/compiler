/**
 * Kits this compiler was not made for give a clear error (lib/compat.mjs): a kit.api.json of another schema, an SDK
 * checkout missing what the build uses, an SDK of another project format. Each one is the issue kits_incompatible,
 * through the command too.
 */
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { mkdirSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { checkKits, checkSdk, sceneFormatVersion, sdkPin } from '../lib/compat.mjs';
import { build } from '../lib/build.mjs';
import { main } from '../lib/cli.mjs';
import { ROOM_DIR, ROOT, kits, projectDir, room, tmp } from './helpers.mjs';

const compat = sdkPin().compat;

/** A fake SDK: the files the build uses, the format `version` (in the crate or the workspace). */
function sdk(version, { inWorkspace = false, drop = [] } = {}) {
  const d = tmp();
  for (const p of compat.paths.filter((x) => !drop.includes(x))) {
    mkdirSync(join(d, p, '..'), { recursive: true });
    writeFileSync(join(d, p), '');
  }
  if (!drop.includes('core/scene-format/Cargo.toml')) {
    writeFileSync(join(d, 'core/scene-format/Cargo.toml'), inWorkspace ? '[package]\nname = "gripforge-scene-format"\nversion.workspace = true\n' : `[package]\nname = "gripforge-scene-format"\nversion = "${version}"\n`);
  }
  if (inWorkspace) writeFileSync(join(d, 'Cargo.toml'), `[workspace]\nmembers = []\n\n[workspace.package]\nversion = "${version}"\nedition = "2021"\n`);
  return d;
}

test('the pin says which kits the compiler is made for', () => {
  const pin = sdkPin();
  assert.match(pin.rev, /^[0-9a-f]{40}$/);
  assert.equal(compat.kitApiSchema, 'gripforge.kit-api/1');
  assert.throws(() => sdkPin(['/nowhere/sdk.json']), /no sdk.json/);
});

test('the kits of the SDK fit; a kit of another schema does not', () => {
  assert.deepEqual(checkKits(kits()), []);
  const [i] = checkKits([{ id: 'core.light', schema: 'gripforge.kit-api/2' }, { id: 'x', schema: compat.kitApiSchema }]);
  assert.equal(i.code, 'kits_incompatible');
  assert.match(i.message, /core\.light describes itself with "gripforge.kit-api\/2", this compiler reads gripforge.kit-api\/1/);
  assert.equal(checkKits([{}])[0].property, 'kits.?.schema');
});

test('the SDK: its own format version, the workspace one, another one, missing files', () => {
  assert.deepEqual(checkSdk(ROOT), []);
  assert.equal(sceneFormatVersion(sdk(`${compat.sceneFormat}.7`)), `${compat.sceneFormat}.7`);
  assert.deepEqual(checkSdk(sdk(`${compat.sceneFormat}.7`, { inWorkspace: true })), []);
  const [other] = checkSdk(sdk('9.4.0'));
  assert.equal(other.code, 'kits_incompatible');
  assert.match(other.message, /carries the project format 9\.4\.0 .* built for/);
  assert.match(other.fix, /made for the format 9\.4/);
  const [missing] = checkSdk(sdk('0', { drop: ['tools/kit-modules/build.mjs'] }));
  assert.match(missing.message, /lacks tools\/kit-modules\/build\.mjs/);
  assert.equal(sceneFormatVersion(tmp()), null);
  const noWs = sdk('0', { inWorkspace: true });
  writeFileSync(join(noWs, 'Cargo.toml'), '[workspace]\n');
  assert.equal(sceneFormatVersion(noWs), null);
  assert.match(checkSdk(sdk('0', { drop: ['core/scene-format/Cargo.toml'] }))[0].message, /lacks core\/scene-format\/Cargo.toml/);
});

test('the command refuses incompatible kits and SDK with kits_incompatible, nothing built', async () => {
  const kitsDir = tmp();
  mkdirSync(join(kitsDir, 'core_light'));
  writeFileSync(join(kitsDir, 'core_light', 'kit.api.json'), JSON.stringify({ id: 'core.light', schema: 'gripforge.kit-api/0' }));
  const r = await build({ project: projectDir(room()), kits: kitsDir, compile: false });
  assert.equal(r.ok, false);
  assert.equal(r.issues[0].code, 'kits_incompatible');
  const old = await build({ project: ROOM_DIR, sdk: sdk('9.0.0'), out: tmp() });
  assert.equal(old.ok, false);
  assert.match(old.issues[0].message, /project format 9\.0\.0/);
  const err = [];
  assert.equal(await main([projectDir(room()), '--check', '--kits', kitsDir], { out: () => {}, err: (s) => err.push(s) }), 1);
  assert.match(err.join('\n'), /kits_incompatible/);
});
