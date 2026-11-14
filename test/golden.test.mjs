/**
 * Golden tests: what the compiler writes for the dev.builder room, frozen in test/golden/ (the paths of the SDK
 * replaced by <SDK>), and the messages it gives for a broken project. A difference fails with a line diff (- golden,
 * + generated); a wanted change is written by `UPDATE_GOLDEN=1 npm test` and reviewed in the PR.
 */
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import { build } from '../lib/build.mjs';
import { main } from '../lib/cli.mjs';
import { KITS, ROOM_DIR, ROOT, diff, golden, node, projectDir, room, scene, tmp } from './helpers.mjs';

const FILES = ['game.json', 'game/Cargo.toml', 'game/kit.api.json', 'game/module.json', 'game/src/lib.rs', 'game/src/index.rs',
  'game/src/init_scene.rs', 'game/src/init_scene/menu.rs', 'game/src/init_scene/level.rs'];

test('the room: game.json, the crate and the generated code are the golden ones', async () => {
  const out = tmp();
  const r = await build({ project: ROOM_DIR, out, compile: false });
  assert.equal(r.ok, true, JSON.stringify(r.issues, null, 1));
  for (const f of FILES) {
    const text = readFileSync(join(out, f), 'utf8').split(ROOT).join('<SDK>');
    golden(`room/${f.replace(/\//g, '__')}`, text);
  }
});

test('the messages for a broken project are the golden ones', async () => {
  const p = room();
  scene(p, 'level').kits['core.light'].brightness = 2;
  scene(p, 'level').kits['core.light'].exposure = -1;
  node(scene(p, 'level'), 'Sun').light.intensity = 'strong';
  node(scene(p, 'level'), 'Shelf').scene = 'scenes/cupboard.gfscene';
  scene(p, 'menu').on[0].do[0].load_scene.scene = 'credits';
  p.installed.push({ id: 'world.nowhere' });
  p.data.scenes.push({ id: 'void', nodes: [] });
  const err = [];
  const code = await main([projectDir(p), '--check', '--kits', KITS], { out: () => {}, err: (s) => err.push(s) });
  assert.equal(code, 1);
  golden('broken-project.txt', `${err.join('\n')}\n`);
});

test('the diff of a golden file is readable: the line, - expected, + generated', () => {
  const d = diff('a\nb\nc\nd\ne\nf\ng\nh\ni', 'a\nb\nc\nD\ne\nf\ng\nh\ni');
  assert.match(d, /^- +4 \| d$/m);
  assert.match(d, /^\+ +\d+ \| D$/m);
  assert.match(d, /^  +3 \| c$/m);
  assert.match(d, /^  \.\.\.$/m);
  assert.equal(diff('same', 'same'), '');
});
