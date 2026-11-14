/**
 * Steps 3 and 4, references and the call of the generator (the core's `code`, core/scene-format `code::code`): the
 * files it writes, the references between elements as Rust variables, the order of posing (parents first, the kits
 * in their `after` order whatever the order written), the build's game.json, a refusal of the generator itself.
 */
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { call } from '../lib/core.mjs';
import { kits, node, room, scene } from './helpers.mjs';

const code = (project, extra = {}, catalog = kits()) => call({ op: 'code', project, files: {}, kits: catalog, ...extra });
const file = (a, path) => a.files.find((f) => f.path === path)?.text ?? assert.fail(`no ${path} in ${a.files.map((f) => f.path)}`);

test('the generator writes init_scene.rs and one module per scene; the entry index.rs calls', () => {
  const a = code(room());
  assert.equal(a.ok, true, JSON.stringify(a.issues, null, 1));
  assert.deepEqual(a.files.map((f) => f.path), ['init_scene.rs', 'init_scene/menu.rs', 'init_scene/level.rs']);
  assert.equal(a.entry, 'init_scene::enter');
  assert.match(file(a, 'init_scene.rs'), /pub fn enter\(h: &mut \(impl Host \+ \?Sized\), id: &str\) -> Option<Posed>/);
});

test('references between elements are Rust variables: a child names its parent by its variable', () => {
  const level = file(code(room()), 'init_scene/level.rs');
  const hall = /let (n\d+_hall) = p\.node\(h, 0, "Hall"/.exec(level)?.[1];
  assert.ok(hall, level);
  assert.match(level, new RegExp(`p\\.node\\(h, ${hall}, "Hall/Crate", "Crate"`));
});

test('the nodes are posed parents first, in the order of the project', () => {
  const level = file(code(room()), 'init_scene/level.rs');
  const order = [...level.matchAll(/p\.node\(h, \w+, "([^"]+)"/g)].map((m) => m[1]);
  assert.deepEqual(order.slice(0, 5), ['Floor', 'Hall', 'Hall/Crate', 'Visitor', 'Door']);
  for (const path of order) {
    const parent = path.split('/').slice(0, -1).join('/');
    if (parent && order.includes(parent)) assert.ok(order.indexOf(parent) < order.indexOf(path), `${parent} before ${path}`);
  }
});

test('the kits\' elements are made in their `after` order, whatever the order written', () => {
  const base = { id: 'test.base', version: '1.0.0', expose: [{ id: 'test.base.ground', for: ['scene'], at: 'scene', props: { seed: { type: 'f64', default: 0 } }, create: 'test.base.elementCreate', remove: 'test.base.elementRemove' }] };
  const top = { id: 'test.top', version: '1.0.0', expose: [{ id: 'test.top.trees', for: ['scene'], at: 'scene', after: ['test.base'], props: { count: { type: 'f64', default: 1 } }, create: 'test.top.elementCreate', remove: 'test.top.elementRemove' }] };
  const p = room();
  p.installed.push({ id: 'test.base' }, { id: 'test.top' });
  const m = scene(p, 'menu');
  m.kits = { 'test.top': { count: 3 }, 'test.base': { seed: 7 } };
  m.requires.push('test.top', 'test.base');
  const a = code(p, {}, [...kits(), base, top]);
  assert.equal(a.ok, true, JSON.stringify(a.issues, null, 1));
  const menu = file(a, 'init_scene/menu.rs');
  const b = menu.indexOf('test_base::ground::create'), t = menu.indexOf('test_top::trees::create');
  assert.ok(b > 0 && t > 0, menu);
  assert.ok(b < t, 'test.base (named in after) is made before test.top');
  assert.match(menu, /count: 3\.0/);
  assert.match(menu, /seed: 7\.0/);
});

test('the values are typed in the code: a section\'s properties are the fields of its Props', () => {
  const level = file(code(room()), 'init_scene/level.rs');
  assert.match(level, /core_light::ambience::Props \{/);
  assert.match(level, /exposure: 1\.2/);
});

test('a scene that is not posed by code (refused at the check) gives no file', () => {
  const p = room();
  node(scene(p, 'level'), 'Shelf').scene = 'scenes/cupboard.gfscene';
  const a = code(p);
  assert.equal(a.ok, false);
  assert.equal(a.files, undefined);
});

test('the generator\'s own refusal is a located error (a broken reference let through by --lenient)', () => {
  const p = room();
  node(scene(p, 'level'), 'Shelf').scene = 'scenes/cupboard.gfscene';
  const a = code(p, { lenient: true });
  assert.equal(a.ok, false);
  const i = a.issues.find((x) => x.level === 'error');
  assert.equal(i.code, 'scene_file_missing');
  assert.equal(i.promoted, true);
  assert.equal(i.scene, 'level');
});

test('pack: the game.json of a built game (scenes lowered, dev.builder out, game.main last, the build stamped)', () => {
  const a = call({ op: 'pack', project: room(), files: {}, kits: kits() });
  assert.equal(a.ok, true);
  const g = a.game;
  assert.deepEqual(g.lock.order, ['game.engine', 'core.light', 'core.camera', 'game.main']);
  assert.equal(g.lock.kits['dev.builder'], undefined);
  assert.ok(!g.project.installed.some((k) => k.id === 'dev.builder'));
  assert.ok(!g.project.requires.includes('dev.builder'));
  assert.equal(g.project.data.scene_files, undefined, 'no scene file is resolved at run time');
  assert.ok(g.project.data.scenes.every((s) => !s.nodes));
  assert.ok(scene(g.project, 'level').instances.length > 0);
  assert.deepEqual(g.build, { schema: 'gripforge.build/1', compiler: g.build.compiler, gameModule: 'game.main', scenes: ['menu', 'level'] });
});

test('pack refuses what check refuses, and a kit whose version nobody knows', () => {
  const p = room();
  p.data.scenes = [];
  assert.equal(call({ op: 'pack', project: p, files: {}, kits: kits() }).ok, false);
  const q = room();
  const noVersion = kits().map((k) => (k.id === 'core.camera' ? { ...k, version: undefined } : k));
  const a = call({ op: 'pack', project: q, files: {}, kits: noVersion });
  assert.equal(a.ok, false);
  assert.ok(a.issues.some((i) => i.code === 'kit_version_unknown'), JSON.stringify(a.issues));
});

test('a scene id that is not a Rust name gets a module name', () => {
  const p = room();
  p.data.scenes.push({ id: '2nd-floor', nodes: [{ name: 'A', type: 'group' }] });
  const a = call({ op: 'check', project: p, files: {}, kits: kits() });
  assert.deepEqual(a.scenes.at(-1), { id: '2nd-floor', file: '#2nd-floor', module: 'scene_2nd_floor' });
});
